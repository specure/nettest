//! Browser WASM driver — the "real handlers" path.
//!
//! Runs the RMBT greeting → ping → download phases **in Rust (wasm)** by driving
//! the SAME handler functions the native client uses (`client::handlers::*` via
//! `basic_handler`), over a `Stream::Js(JsWss)` WebSocket. A browser has no
//! blocking `poll()`, so this async `pump` replaces the native mio event loop:
//! it looks at the stream's current readiness interest and either runs the
//! writable handler (send) or, once data has arrived (awaited via the WebSocket
//! `onmessage` waker), the readable handler. Handlers stay unchanged — they read
//! non-blocking (`WouldBlock` when the inbox is empty) exactly as over a socket.
//!
//! Jitter/packet-loss are UDP-only and omitted.

use std::task::Poll as TaskPoll;

use futures::future::poll_fn;
use wasm_bindgen::prelude::*;
use web_time::Instant;

use crate::client::calculator::calculate_download_speed_from_stats_silent;
use crate::client::handlers::basic_handler::{
    handle_client_readable_data, handle_client_writable_data,
};
use crate::client::state::{MeasurementState, TestPhase};
use crate::reactor::{Interest, Poll, Token};
use crate::stream::js_wss::{JsWss, Notify};
use crate::stream::stream::Stream;

fn log(f: &js_sys::Function, msg: &str) {
    let _ = f.call1(&JsValue::NULL, &JsValue::from_str(msg));
}

async fn await_open(n: &Notify) -> Result<(), JsValue> {
    poll_fn(|cx| {
        if n.is_open() {
            TaskPoll::Ready(Ok(()))
        } else if n.is_closed() {
            TaskPoll::Ready(Err(JsValue::from_str("socket closed before open")))
        } else {
            n.set_waker(cx.waker());
            TaskPoll::Pending
        }
    })
    .await
}

async fn await_readable(n: &Notify) -> Result<(), JsValue> {
    poll_fn(|cx| {
        if n.has_incoming() {
            TaskPoll::Ready(Ok(()))
        } else if n.is_closed() {
            TaskPoll::Ready(Err(JsValue::from_str("socket closed")))
        } else {
            n.set_waker(cx.waker());
            TaskPoll::Pending
        }
    })
    .await
}

fn interest(state: &MeasurementState) -> Interest {
    match &state.stream {
        Stream::Js(s) => s.interest(),
    }
}

fn set_writable(state: &mut MeasurementState, poll: &Poll) {
    let token = state.token;
    match &mut state.stream {
        Stream::Js(s) => {
            let _ = s.reregister(poll, token, Interest::WRITABLE);
        }
    }
}

/// Drive the shared handlers until `state.phase == target`. This is the async
/// analogue of the native `TestState::process_phase` loop.
async fn pump_until(
    state: &mut MeasurementState,
    notify: &Notify,
    poll: &Poll,
    target: TestPhase,
) -> Result<(), JsValue> {
    loop {
        if state.phase == target {
            return Ok(());
        }
        let r = if interest(state).is_writable() {
            handle_client_writable_data(state, poll)
        } else {
            if !notify.has_incoming() {
                await_readable(notify).await?;
                continue;
            }
            handle_client_readable_data(state, poll)
        };
        match r {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                await_readable(notify).await?;
            }
            Err(e) => return Err(JsValue::from_str(&format!("handler error: {e}"))),
        }
    }
}

/// Run greeting → ping → download entirely in Rust/wasm, driving the real RMBT
/// handlers over a browser WebSocket. `log_fn(msg: string)` receives progress.
/// Resolves to `{ pingMs, downloadMbps, downloadSamples }`.
#[wasm_bindgen]
pub async fn run_measurement(url: String, log_fn: js_sys::Function) -> Result<JsValue, JsValue> {
    let poll = Poll::new().map_err(|e| JsValue::from_str(&e.to_string()))?;

    let wss = JsWss::connect(&url)?;
    let notify = wss.notify();
    let dummy_addr = std::net::SocketAddr::from(([0u8, 0, 0, 0], 0));
    let mut state = MeasurementState::new(Stream::Js(wss), Token(1), dummy_addr);

    await_open(&notify).await?;
    log(&log_fn, &format!("connected {url} (real Rust/wasm handlers)"));

    // ---- GREETING (RMBTv / ACCEPT TOKEN / TOKEN / OK / CHUNKSIZE / ACCEPT) ----
    state.phase_start_time = Some(Instant::now());
    pump_until(&mut state, &notify, &poll, TestPhase::GreetingCompleted).await?;
    log(&log_fn, "greeting: completed");

    // ---- PING (kick like the native run_ping) ----
    state.phase = TestPhase::PingSendPing;
    set_writable(&mut state, &poll);
    state.phase_start_time = Some(Instant::now());
    pump_until(&mut state, &notify, &poll, TestPhase::PingCompleted).await?;
    let ping_ms = state.ping_median.map(|ns| ns as f64 / 1e6).unwrap_or(f64::NAN);
    log(&log_fn, &format!("ping: {ping_ms:.2} ms"));

    // ---- DOWNLOAD (GETTIME; kick like the native run_get_time) ----
    state.download_duration_ms = 2000;
    state.phase = TestPhase::GetTimeSendCommand;
    set_writable(&mut state, &poll);
    state.phase_start_time = Some(Instant::now());
    pump_until(&mut state, &notify, &poll, TestPhase::GetTimeCompleted).await?;
    let stats = vec![state.download_measurements.iter().cloned().collect::<Vec<_>>()];
    let (bps, _gbps, _) = calculate_download_speed_from_stats_silent(&stats);
    let mbps = bps / 1e6;
    log(
        &log_fn,
        &format!(
            "download: {mbps:.2} Mbit/s ({} samples, {} bytes)",
            state.download_measurements.len(),
            state.bytes_received
        ),
    );

    if let Stream::Js(s) = &mut state.stream {
        let _ = s.close();
    }

    let obj = js_sys::Object::new();
    js_sys::Reflect::set(&obj, &"pingMs".into(), &ping_ms.into())?;
    js_sys::Reflect::set(&obj, &"downloadMbps".into(), &mbps.into())?;
    js_sys::Reflect::set(
        &obj,
        &"downloadSamples".into(),
        &(state.download_measurements.len() as f64).into(),
    )?;
    Ok(obj.into())
}
