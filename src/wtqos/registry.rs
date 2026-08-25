//! Shared state between the QUIC session tasks (tokio) and the RMBT control
//! connections (mio worker threads).
//!
//! A control connection only ever knows a session by the 16-byte UUID it handed
//! the client, so the registry is keyed by that UUID: the client registers its
//! QUIC session with the same UUID, and from then on `WTTEST`/`GET WTRESULT` on
//! the control channel reach the right datagram session. Control handlers must
//! never block a mio worker, so they only read counters and push commands into
//! a channel the session task drains.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use tokio::sync::mpsc::UnboundedSender;

/// What a control connection can ask a live QUIC session to do.
#[derive(Debug, Clone, Copy)]
pub enum SessionCommand {
    /// Start counting an incoming (client→server) train of `expected` packets.
    ArmOut { expected: u32 },
    /// Send a server→client train: `count` datagrams, one every `delay_ms`.
    SendTrain { count: u32, delay_ms: u64 },
}

/// Per-direction counters, filled by the session task and read by the control
/// connection when the client asks for the result.
#[derive(Debug, Default, Clone, Copy)]
pub struct DirectionStats {
    /// Packets the peer was told to send (OUT: expected, IN: what we sent).
    pub expected: u32,
    /// Distinct sequence numbers seen (OUT) / datagrams handed to QUIC (IN).
    pub counted: u32,
    /// Same sequence number seen more than once.
    pub duplicates: u32,
    /// Packets that arrived after a higher sequence number.
    pub out_of_order: u32,
    /// RFC 3550 interarrival jitter in nanoseconds (OUT only).
    pub jitter_ns: u64,
    /// Largest single transit-time difference in nanoseconds (OUT only).
    pub max_delta_ns: u64,
    /// Whether the train this describes has finished.
    pub done: bool,
}

/// One registered QUIC session.
pub struct WtSession {
    commands: UnboundedSender<SessionCommand>,
    pub out: Mutex<DirectionStats>,
    pub inbound: Mutex<DirectionStats>,
}

impl WtSession {
    pub fn send(&self, command: SessionCommand) -> bool {
        self.commands.send(command).is_ok()
    }
}

/// Sessions by UUID, plus what a control connection needs to describe the
/// endpoint to a client.
pub struct WtRegistry {
    sessions: Mutex<HashMap<[u8; 16], Arc<WtSession>>>,
    /// Port the QUIC endpoint listens on.
    pub port: u16,
    /// Base64 SHA-256 of the leaf certificate. Always published: a client that
    /// cannot verify the chain needs it for `serverCertificateHashes`.
    pub cert_hash_base64: Option<String>,
    /// Whether that certificate is self-signed, i.e. whether the hash is
    /// *required* rather than merely informative.
    pub self_signed: bool,
}

impl WtRegistry {
    pub fn new(port: u16, cert_hash_base64: Option<String>, self_signed: bool) -> WtRegistry {
        WtRegistry {
            sessions: Mutex::new(HashMap::new()),
            port,
            cert_hash_base64,
            self_signed,
        }
    }

    pub fn insert(&self, uuid: [u8; 16], commands: UnboundedSender<SessionCommand>) -> Arc<WtSession> {
        let session = Arc::new(WtSession {
            commands,
            out: Mutex::new(DirectionStats::default()),
            inbound: Mutex::new(DirectionStats::default()),
        });
        if let Ok(mut sessions) = self.sessions.lock() {
            sessions.insert(uuid, session.clone());
        }
        session
    }

    pub fn get(&self, uuid: &[u8; 16]) -> Option<Arc<WtSession>> {
        self.sessions.lock().ok()?.get(uuid).cloned()
    }

    pub fn remove(&self, uuid: &[u8; 16]) {
        if let Ok(mut sessions) = self.sessions.lock() {
            sessions.remove(uuid);
        }
    }

    pub fn len(&self) -> usize {
        self.sessions.lock().map(|s| s.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

static REGISTRY: OnceLock<Arc<WtRegistry>> = OnceLock::new();

/// The process-wide registry, or `None` when the endpoint is disabled or failed
/// to start — control handlers use that to answer "QoS unavailable".
pub fn registry() -> Option<&'static Arc<WtRegistry>> {
    REGISTRY.get()
}

/// Publish the registry. Returns false if one was already installed.
pub fn install(registry: Arc<WtRegistry>) -> bool {
    REGISTRY.set(registry).is_ok()
}

/// Parse the hex UUID that the control channel carries (32 hex chars), the same
/// encoding the native `UDPTEST` commands use.
pub fn uuid_from_hex(hex: &str) -> Option<[u8; 16]> {
    let hex = hex.trim();
    if hex.len() != 32 {
        return None;
    }
    let mut uuid = [0u8; 16];
    for (i, byte) in uuid.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(uuid)
}

#[cfg(test)]
mod tests {
    use super::uuid_from_hex;

    #[test]
    fn parses_a_32_char_hex_uuid() {
        let uuid = uuid_from_hex("000102030405060708090a0b0c0d0e0f").unwrap();
        assert_eq!(uuid, [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]);
    }

    #[test]
    fn rejects_wrong_length_or_non_hex() {
        assert!(uuid_from_hex("00010203").is_none());
        assert!(uuid_from_hex("zz0102030405060708090a0b0c0d0e0f").is_none());
    }
}
