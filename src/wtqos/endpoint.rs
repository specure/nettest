//! The QUIC/WebTransport endpoint: accepts sessions and runs the datagram
//! trains that measure jitter and packet loss.
//!
//! One tokio task per session handles both directions at once (`select!`):
//! incoming datagrams update the OUT statistics, and commands from the control
//! connection start the server→client (IN) train.

use std::net::Ipv6Addr;
use std::sync::Arc;
use std::time::Duration;

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use log::{debug, info, warn};
use tokio::sync::mpsc::unbounded_channel;
use wtransport::endpoint::IncomingSession;
use wtransport::{Endpoint, Identity, ServerConfig};

use crate::udp::payload::{UdpPayload, FLAG_HOLE_PUNCH, FLAG_ONE_DIRECTION, FLAG_RESPONSE};
use crate::wtqos::registry::{install, DirectionStats, SessionCommand, WtRegistry};
use crate::wtqos::WT_PATH;

/// Idle timeout for a QoS session — a QoS phase is a few seconds; anything
/// longer is an abandoned tab.
const IDLE_TIMEOUT: Duration = Duration::from_secs(30);

/// Start the endpoint on `port` and publish its registry.
///
/// `identity` is either built from the configured certificate or self-signed;
/// in the self-signed case the leaf hash is published so a browser can accept it
/// through `serverCertificateHashes`. Must be called from a tokio runtime.
pub fn start(port: u16, identity: Identity, self_signed: bool) -> anyhow::Result<()> {
    // Always publish the leaf hash — it is public information from the TLS
    // handshake, and an operator running a self-signed certificate from
    // `cert_path` would otherwise leave browsers with no way to connect. The
    // `self_signed` flag travels with it so a client knows whether it *needs*
    // the hash (`serverCertificateHashes` only accepts short-lived P-256 certs,
    // so it must not be used against a normally trusted chain).
    let cert_hash = identity
        .certificate_chain()
        .as_slice()
        .first()
        .map(|cert| {
            let digest: [u8; 32] = *cert.hash().as_ref();
            BASE64.encode(digest)
        });

    let config = ServerConfig::builder()
        .with_bind_default(port)
        .with_identity(identity)
        .max_idle_timeout(Some(IDLE_TIMEOUT))?
        .keep_alive_interval(Some(Duration::from_secs(3)))
        .build();

    let endpoint = Endpoint::server(config)?;
    let registry = Arc::new(WtRegistry::new(port, cert_hash.clone(), self_signed));
    if !install(registry.clone()) {
        warn!("WebTransport registry already installed; endpoint not started twice");
        return Ok(());
    }

    info!(
        "WebTransport QoS endpoint listening on UDP {} (path {}), {} cert sha256 {}",
        port,
        WT_PATH,
        if self_signed { "self-signed" } else { "configured" },
        cert_hash.as_deref().unwrap_or("unavailable")
    );

    tokio::spawn(async move {
        loop {
            let incoming = endpoint.accept().await;
            tokio::spawn(async move {
                if let Err(e) = serve_session(incoming).await {
                    debug!("WebTransport session ended: {e}");
                }
            });
        }
    });
    Ok(())
}

/// Build an identity from configured cert/key paths, falling back to a
/// self-signed one. The bool in the result says whether it is self-signed, i.e.
/// whether clients need the certificate hash to connect.
pub async fn identity_from_config(
    cert_path: Option<&str>,
    key_path: Option<&str>,
) -> anyhow::Result<(Identity, bool)> {
    if let (Some(cert), Some(key)) = (cert_path, key_path) {
        match Identity::load_pemfiles(cert, key).await {
            Ok(identity) => return Ok((identity, false)),
            Err(e) => warn!("WebTransport: cannot load {cert}/{key} ({e}); using a self-signed identity"),
        }
    }
    // 14-day ECDSA P-256 — the shape Chrome requires for
    // `serverCertificateHashes`.
    let identity = Identity::self_signed([
        "localhost",
        "127.0.0.1",
        &Ipv6Addr::LOCALHOST.to_string(),
    ])?;
    Ok((identity, true))
}

async fn serve_session(incoming: IncomingSession) -> anyhow::Result<()> {
    let request = incoming.await?;
    if request.path() != WT_PATH {
        debug!("WebTransport: rejecting path {}", request.path());
        request.not_found().await;
        return Ok(());
    }
    let connection = request.accept().await?;
    debug!("WebTransport session accepted, id {}", connection.stable_id());

    let (tx, mut commands) = unbounded_channel::<SessionCommand>();
    // The session is only reachable from the control channel once the client
    // registers a UUID (its first datagram), so keep the channel until then.
    let mut session = None;
    let mut uuid = None;
    // RFC 3550 §6.4.1 accumulated over transit times (arrival − client send).
    // The constant clock offset between the two machines cancels in the
    // consecutive differences, so unsynchronised clocks are fine.
    //
    // Accumulated per packet rather than in a final pass: a train that loses
    // packets never reaches its expected count, and the control channel must
    // still find a usable figure when it asks for the result.
    let mut jitter = Jitter::default();
    let mut highest_seq: i64 = -1;
    let mut seen: std::collections::HashSet<u32> = std::collections::HashSet::new();

    loop {
        tokio::select! {
            datagram = connection.receive_datagram() => {
                let datagram = match datagram {
                    Ok(d) => d,
                    Err(e) => {
                        if let (Some(id), Some(s)) = (uuid, &session) {
                            mark_out_done(s);
                            if let Some(registry) = crate::wtqos::registry() {
                                registry.remove(&id);
                            }
                        }
                        return Err(e.into());
                    }
                };
                let payload = match UdpPayload::from_bytes(&datagram.payload()) {
                    Some(p) => p,
                    None => continue,
                };
                match payload.communication_flag {
                    // Registration: bind this QUIC session to the UUID the
                    // control channel knows, and echo so the client can tell
                    // the binding is live before it starts a test.
                    FLAG_HOLE_PUNCH => {
                        if session.is_none() {
                            if let Some(registry) = crate::wtqos::registry() {
                                session = Some(registry.insert(payload.uuid, tx.clone()));
                                uuid = Some(payload.uuid);
                                debug!("WebTransport session registered, {} live", registry.len());
                            }
                        }
                        let echo = UdpPayload {
                            communication_flag: FLAG_RESPONSE,
                            packet_number: payload.packet_number,
                            uuid: payload.uuid,
                            timestamp_ns: payload.timestamp_ns,
                        };
                        let _ = connection.send_datagram(echo.to_bytes().to_vec());
                    }
                    FLAG_ONE_DIRECTION => {
                        let Some(s) = &session else { continue };
                        let arrival = now_ns();
                        let mut stats = match s.out.lock() {
                            Ok(g) => g,
                            Err(_) => continue,
                        };
                        if !seen.insert(payload.packet_number) {
                            stats.duplicates += 1;
                            continue;
                        }
                        stats.counted += 1;
                        if (payload.packet_number as i64) < highest_seq {
                            stats.out_of_order += 1;
                        } else {
                            highest_seq = payload.packet_number as i64;
                        }
                        // Signed: the client's clock may be ahead of ours by
                        // seconds, and that offset must survive into the
                        // differences, where it cancels.
                        let transit = arrival as i64 - payload.timestamp_ns;
                        jitter.push(transit);
                        stats.jitter_ns = jitter.value_ns();
                        stats.max_delta_ns = jitter.max_delta_ns;
                        if stats.expected > 0 && stats.counted >= stats.expected {
                            stats.done = true;
                        }
                    }
                    _ => {}
                }
            }
            command = commands.recv() => {
                let Some(command) = command else { continue };
                match command {
                    SessionCommand::ArmOut { expected } => {
                        jitter = Jitter::default();
                        seen.clear();
                        highest_seq = -1;
                        if let Some(s) = &session {
                            if let Ok(mut stats) = s.out.lock() {
                                *stats = DirectionStats { expected, ..Default::default() };
                            }
                        }
                    }
                    SessionCommand::SendTrain { count, delay_ms } => {
                        if let Some(s) = &session {
                            if let Ok(mut stats) = s.inbound.lock() {
                                *stats = DirectionStats { expected: count, ..Default::default() };
                            }
                        }
                        for seq in 0..count {
                            let packet = UdpPayload {
                                communication_flag: FLAG_ONE_DIRECTION,
                                packet_number: seq,
                                uuid: uuid.unwrap_or([0u8; 16]),
                                timestamp_ns: now_ns() as i64,
                            };
                            if connection.send_datagram(packet.to_bytes().to_vec()).is_err() {
                                break;
                            }
                            if let Some(s) = &session {
                                if let Ok(mut stats) = s.inbound.lock() {
                                    stats.counted = seq + 1;
                                }
                            }
                            if seq + 1 < count {
                                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                            }
                        }
                        if let Some(s) = &session {
                            if let Ok(mut stats) = s.inbound.lock() {
                                stats.done = true;
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Running RFC 3550 interarrival jitter — the same smoothing
/// [`crate::client::calculator::rfc3550_jitter_ns`] applies to a finished
/// series, kept up to date packet by packet.
#[derive(Default)]
struct Jitter {
    smoothed: f64,
    previous_transit: Option<i64>,
    max_delta_ns: u64,
}

impl Jitter {
    fn push(&mut self, transit_ns: i64) {
        if let Some(previous) = self.previous_transit {
            let delta = transit_ns.abs_diff(previous);
            self.smoothed += (delta as f64 - self.smoothed) / 16.0;
            self.max_delta_ns = self.max_delta_ns.max(delta);
        }
        self.previous_transit = Some(transit_ns);
    }

    fn value_ns(&self) -> u64 {
        self.smoothed.round().max(0.0) as u64
    }
}

/// The peer went away: whatever we have is the final answer.
fn mark_out_done(session: &Arc<crate::wtqos::registry::WtSession>) {
    if let Ok(mut stats) = session.out.lock() {
        stats.done = true;
    }
}

fn now_ns() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::Jitter;

    /// The client's clock can sit seconds away from ours, in either direction.
    /// That offset must not reach the jitter figure.
    #[test]
    fn jitter_survives_a_clock_offset() {
        let series = [20_000_000i64, 21_000_000, 19_500_000, 20_500_000, 20_100_000];
        let run = |offset: i64| {
            let mut jitter = Jitter::default();
            for t in series {
                jitter.push(t + offset);
            }
            jitter.value_ns()
        };
        let baseline = run(0);
        for offset in [-5_000_000_000i64, -1_330_000_000, 1_330_000_000] {
            assert_eq!(run(offset), baseline, "offset {offset} changed the jitter");
        }
        assert!(baseline > 0, "a varying series must not report zero jitter");
    }
}
