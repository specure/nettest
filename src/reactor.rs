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

// Browser stand-ins mirroring the slice of the mio surface the client uses, so
// the shared handler code type-checks on wasm once `Stream` gains a JS variant.
// There is no OS poll in the browser: `Poll` is inert and the JS event loop
// drives a pump; `register`/`reregister` on a JS stream just record interest.
#[cfg(target_arch = "wasm32")]
mod wasm_shim {
    #[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
    pub struct Token(pub usize);

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Interest(u8);
    impl Interest {
        pub const READABLE: Interest = Interest(0b01);
        pub const WRITABLE: Interest = Interest(0b10);
        pub fn is_readable(&self) -> bool {
            self.0 & 0b01 != 0
        }
        pub fn is_writable(&self) -> bool {
            self.0 & 0b10 != 0
        }
    }
    impl std::ops::BitOr for Interest {
        type Output = Interest;
        fn bitor(self, rhs: Interest) -> Interest {
            Interest(self.0 | rhs.0)
        }
    }

    /// Inert stand-in for `mio::Poll`. Only exists so `&Poll` arguments in the
    /// shared handlers type-check; the wasm driver never calls a real poll.
    pub struct Poll;
    impl Poll {
        pub fn new() -> std::io::Result<Poll> {
            Ok(Poll)
        }
    }

    /// Placeholder for `mio::Events` (the wasm pump does not iterate events).
    pub struct Events;
    impl Events {
        pub fn with_capacity(_capacity: usize) -> Events {
            Events
        }
    }
}
#[cfg(target_arch = "wasm32")]
pub use wasm_shim::{Events, Interest, Poll, Token};
