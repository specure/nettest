//! Reactor abstraction — the interface that decouples the RMBT client state
//! machine from its concrete readiness source.
//!
//! Natively the state machine is driven by a mio `Poll` (epoll/kqueue): it
//! registers each socket's interest (READABLE/WRITABLE) and blocks in
//! `poll.poll()` until the OS reports readiness. A browser has neither raw
//! sockets nor a blocking poll, so the same handlers must instead be driven by
//! JS events (`WebSocket.onmessage` / a send tick) calling into wasm.
//!
//! This module names that boundary. The handlers only ever need to *declare*
//! which readiness a token wants (mio's `register`/`reregister`); everything
//! else is plain `Read`/`Write` on the stream. Capturing that as [`Reactor`]
//! lets a JS-backed reactor stand in for mio on wasm.

/// Which readiness a stream currently wants the reactor to watch.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Readiness {
    pub readable: bool,
    pub writable: bool,
}

impl Readiness {
    pub const READABLE: Readiness = Readiness { readable: true, writable: false };
    pub const WRITABLE: Readiness = Readiness { readable: false, writable: true };
    pub const BOTH: Readiness = Readiness { readable: true, writable: true };
}

/// The narrow interface the state machine needs from its event source: declare
/// (or re-declare) which readiness a token wants to be woken for.
///
/// - Native: backed by mio's `Poll::register`/`reregister`.
/// - Wasm: records the interest so the JS pump knows whether to feed reads
///   (from `onmessage`) or drive writes (on a send tick).
pub trait Reactor {
    fn set_interest(&self, token: usize, interest: Readiness);
}

// On native, re-export the slice of mio the client uses. Routing the handlers'
// `use mio::{...}` through `crate::reactor::{...}` is the mechanical step that
// lets the wasm build swap in the shim below without touching handler logic.
#[cfg(not(target_arch = "wasm32"))]
mod native {
    pub use mio::net::TcpStream;
    pub use mio::{Events, Interest, Poll, Token};
}
#[cfg(not(target_arch = "wasm32"))]
pub use native::*;
