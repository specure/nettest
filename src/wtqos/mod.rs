//! WebTransport (QUIC datagram) QoS endpoint — the browser's substitute for the
//! native UDP jitter / packet-loss tests.
//!
//! A browser has no UDP socket, so the RMBT UDP QoS phases cannot run there. It
//! does have WebTransport: datagrams over QUIC, which — unlike anything carried
//! on the control WebSocket — are *not* retransmitted, so loss and jitter stay
//! observable.
//!
//! The design mirrors the native UDP test rather than inventing a second one:
//!
//! * the same 29-byte wire format ([`crate::udp::payload::UdpPayload`]), so a
//!   packet is a flag, a sequence number, the session UUID and a send timestamp;
//! * the same statistics ([`crate::udp::result`], [`crate::client::calculator`]),
//!   so browser results are directly comparable with native ones;
//! * the same control-channel shape as `UDPTEST OUT/IN` + `GET UDPRESULT`, minus
//!   the ports and NAT hole punching — the QUIC session is already established
//!   and bidirectional.
//!
//! The endpoint listens on its own UDP port (default [`DEFAULT_WT_PORT`]),
//! separate from both the RMBT TCP port and the native UDP QoS port.

pub mod endpoint;
pub mod registry;

pub use endpoint::start;
pub use registry::{registry, DirectionStats, WtRegistry};

/// Default UDP port for the QUIC/WebTransport endpoint. Deliberately adjacent to
/// the RMBT TCP port (5005) and the native UDP QoS port (5004).
pub const DEFAULT_WT_PORT: u16 = 5006;

/// URL path clients connect to.
pub const WT_PATH: &str = "/qos";
