//! Browser QoS phase: jitter and packet loss over WebTransport datagrams.
//!
//! The control channel (WebSocket) arranges the test; the packets themselves go
//! over QUIC, because datagrams are the only thing a browser can send that is
//! not retransmitted — over TCP a lost packet is invisible, so a loss figure
//! measured there would describe the transport rather than the network.
//!
//! The exchange mirrors the native UDP test, and both ends share this crate's
//! wire format and statistics, so a browser result is comparable with a native
//! one rather than merely similar:
//!
//! ```text
//! GET WTURL                     -> WTURL <port> <path> <hash> <selfsigned|trusted>
//! (register the QUIC session under a UUID, wait for the echo)
//! WTTEST OUT <n> <delay> <uuid> -> OK   ; we send n datagrams
//! GET WTRESULT OUT              -> RCV <received> <dup> <ooo> <jitter_ns> <max_delta_ns>
//! WTTEST IN  <n> <delay> <uuid> -> OK   ; the server sends n datagrams
//! GET WTRESULT IN               -> SNT <sent>
//! ```
//!
//! Direction handling follows the accuracy constraints of a browser: for IN the
//! server controls the cadence and we only timestamp arrivals, and for OUT our
//! own (unavoidably irregular) send times ride along in the payload so the
//! server can subtract them. Neither figure is contaminated by `setTimeout`.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::spawn_local;
use web_time::Instant;

use crate::client::calculator::rfc3550_jitter_ns;
use crate::udp::payload::{random_uuid, UdpPayload, FLAG_HOLE_PUNCH, FLAG_ONE_DIRECTION, FLAG_RESPONSE};
use crate::stream::wt_datagrams::{supported, WtDatagrams};
use crate::wasm::{command, js_err, sleep_ms, Conn, Ctx};

/// Extra window for late packets after a train, matching the native
/// `DEFAULT_UDP_TMAX_NS`.
const TMAX_MS: u64 = 1000;
/// Control-command timeout.
const COMMAND_TIMEOUT_MS: u64 = 5000;
/// How long to wait for the registration datagram to come back.
const REGISTER_TIMEOUT_MS: u64 = 3000;

/// One direction of the test.
#[derive(Debug, Clone, Copy, Default)]
pub struct Direction {
    pub sent: u32,
    pub received: u32,
    pub jitter_ms: f64,
}

impl Direction {
    pub fn loss_percent(&self) -> f64 {
        if self.sent == 0 {
            return 0.0;
        }
        100.0 * (self.sent.saturating_sub(self.received)) as f64 / self.sent as f64
    }
}

/// What the phase reports back to the driver.
#[derive(Debug, Clone, Copy)]
pub struct QosResult {
    pub out: Direction,
    pub inbound: Direction,
}

impl QosResult {
    /// The native runner reports the worse of the two directions; do the same so
    /// the numbers mean the same thing on both clients.
    pub fn jitter_ms(&self) -> f64 {
        self.out.jitter_ms.max(self.inbound.jitter_ms)
    }

    pub fn packet_loss_percent(&self) -> f64 {
        self.out.loss_percent().max(self.inbound.loss_percent())
    }
}

/// An arrival recorded by the receive pump.
#[derive(Clone, Copy)]
struct Arrival {
    flag: u8,
    sequence: u32,
    sent_ns: i64,
    arrived_ns: i64,
}

/// Absolute time in nanoseconds.
///
/// `performance.timeOrigin + performance.now()` rather than `Date.now()`: the
/// server subtracts our send times from its arrival times, so sub-millisecond
/// resolution is what keeps a millisecond-scale jitter figure meaningful.
fn epoch_ns() -> i64 {
    let global = js_sys::global();
    if let Ok(performance) = js_sys::Reflect::get(&global, &JsValue::from_str("performance")) {
        let origin = js_sys::Reflect::get(&performance, &JsValue::from_str("timeOrigin"))
            .ok()
            .and_then(|v| v.as_f64());
        let now = js_sys::Reflect::get(&performance, &JsValue::from_str("now"))
            .ok()
            .and_then(|f| f.dyn_into::<js_sys::Function>().ok())
            .and_then(|f| f.call0(&performance).ok())
            .and_then(|v| v.as_f64());
        if let (Some(origin), Some(now)) = (origin, now) {
            return ((origin + now) * 1e6) as i64;
        }
    }
    (js_sys::Date::now() * 1e6) as i64
}

fn hex(uuid: &[u8; 16]) -> String {
    uuid.iter().map(|b| format!("{b:02x}")).collect()
}

/// Host part of the control-channel URL — the QUIC endpoint lives on the same
/// host, on its own UDP port.
fn host_of(url: &str) -> &str {
    let without_scheme = url.split("://").nth(1).unwrap_or(url);
    let host = without_scheme.split('/').next().unwrap_or(without_scheme);
    // Keep a bracketed IPv6 literal intact; otherwise cut the port.
    if host.starts_with('[') {
        host.split(']').next().map(|h| &h[1..]).unwrap_or(host)
    } else {
        host.split(':').next().unwrap_or(host)
    }
}

/// Run the QoS phase on `conn`'s control channel. `Err` means "not measurable
/// here" (no WebTransport, endpoint disabled, UDP blocked) — a caller should
/// fall back rather than fail the measurement.
pub async fn run(
    conn: &mut Conn,
    ctx: &Ctx,
    url: &str,
    packets: u32,
    delay_ms: u64,
) -> Result<QosResult, JsValue> {
    if !supported() {
        return Err(js_err("this browser has no WebTransport"));
    }

    // ---- where is the endpoint, and how do we trust it ----
    let reply = command(conn, "GET WTURL\n", "WTURL", COMMAND_TIMEOUT_MS).await?;
    let fields: Vec<&str> = reply.split_whitespace().collect();
    if fields.len() < 5 {
        return Err(js_err(format!("malformed WTURL reply: {reply}")));
    }
    let port: u16 = fields[1].parse().unwrap_or(0);
    if port == 0 {
        return Err(js_err("server has no WebTransport endpoint"));
    }
    let (path, hash, trust) = (fields[2], fields[3], fields[4]);
    let endpoint = format!("https://{}:{}{}", host_of(url), port, path);

    let session = Rc::new(
        WtDatagrams::connect(
            &endpoint,
            if hash == "-" { None } else { Some(hash) },
            trust == "selfsigned",
        )
        .await?,
    );
    ctx.log(&format!("qos: QUIC session to {endpoint}"));

    // Datagrams are collected by a pump task: `read()` cannot be abandoned
    // mid-flight (the next call would fault on a pending read), so nothing else
    // ever awaits the reader directly.
    let arrivals: Rc<RefCell<Vec<Arrival>>> = Rc::new(RefCell::new(Vec::new()));
    {
        let session = session.clone();
        let arrivals = arrivals.clone();
        spawn_local(async move {
            loop {
                match session.receive().await {
                    Ok(Some(bytes)) => {
                        let arrived_ns = epoch_ns();
                        if let Some(packet) = UdpPayload::from_bytes(&bytes) {
                            arrivals.borrow_mut().push(Arrival {
                                flag: packet.communication_flag,
                                sequence: packet.packet_number,
                                sent_ns: packet.timestamp_ns,
                                arrived_ns,
                            });
                        }
                    }
                    _ => break,
                }
            }
        });
    }

    // ---- bind the QUIC session to the UUID the control channel will name ----
    let uuid = random_uuid();
    let uuid_hex = hex(&uuid);
    session
        .send(
            &UdpPayload {
                communication_flag: FLAG_HOLE_PUNCH,
                packet_number: 0,
                uuid,
                timestamp_ns: epoch_ns(),
            }
            .to_bytes(),
        )
        .await?;
    let deadline = Instant::now() + Duration::from_millis(REGISTER_TIMEOUT_MS);
    loop {
        if arrivals.borrow().iter().any(|a| a.flag == FLAG_RESPONSE) {
            break;
        }
        if Instant::now() >= deadline {
            return Err(js_err("QUIC session registration was not acknowledged"));
        }
        sleep_ms(10).await;
    }
    arrivals.borrow_mut().clear();

    // ---- OUT: client → server ----
    command(
        conn,
        &format!("WTTEST OUT {packets} {delay_ms} {uuid_hex}\n"),
        "OK",
        COMMAND_TIMEOUT_MS,
    )
    .await?;
    for sequence in 0..packets {
        session
            .send(
                &UdpPayload {
                    communication_flag: FLAG_ONE_DIRECTION,
                    packet_number: sequence,
                    uuid,
                    timestamp_ns: epoch_ns(),
                }
                .to_bytes(),
            )
            .await?;
        if sequence + 1 < packets {
            sleep_ms(delay_ms as i32).await;
        }
    }
    sleep_ms(TMAX_MS as i32).await;
    let reply = command(conn, "GET WTRESULT OUT\n", "RCV", COMMAND_TIMEOUT_MS).await?;
    let fields: Vec<&str> = reply.split_whitespace().collect();
    let out = Direction {
        sent: packets,
        received: fields.get(1).and_then(|v| v.parse().ok()).unwrap_or(0),
        jitter_ms: fields
            .get(4)
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(0.0)
            / 1e6,
    };

    // ---- IN: server → client ----
    arrivals.borrow_mut().clear();
    command(
        conn,
        &format!("WTTEST IN {packets} {delay_ms} {uuid_hex}\n"),
        "OK",
        COMMAND_TIMEOUT_MS,
    )
    .await?;
    sleep_ms((packets as u64 * delay_ms + TMAX_MS) as i32).await;
    let reply = command(conn, "GET WTRESULT IN\n", "SNT", COMMAND_TIMEOUT_MS).await?;
    let server_sent: u32 = reply
        .split_whitespace()
        .nth(1)
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);

    let received: Vec<Arrival> = arrivals
        .borrow()
        .iter()
        .filter(|a| a.flag == FLAG_ONE_DIRECTION)
        .copied()
        .collect();
    let mut sequences: Vec<u32> = received.iter().map(|a| a.sequence).collect();
    sequences.sort_unstable();
    sequences.dedup();
    // Transit times carry the clock offset between the two machines, but it is
    // constant and cancels in RFC 3550's consecutive differences.
    let transits: Vec<u64> = received
        .iter()
        .map(|a| a.arrived_ns.saturating_sub(a.sent_ns).max(0) as u64)
        .collect();
    let inbound = Direction {
        sent: server_sent,
        received: sequences.len() as u32,
        jitter_ms: rfc3550_jitter_ns(&transits).unwrap_or(0.0) / 1e6,
    };

    session.close();
    Ok(QosResult { out, inbound })
}
