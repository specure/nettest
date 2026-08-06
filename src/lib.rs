// Native-only modules: raw sockets (mio), TLS, threads, the mio server, UDP
// (jitter/packet-loss), plotting/printing, HTTP control-server client. None of
// these compile for a browser WASM target.
#[cfg(not(target_arch = "wasm32"))]
pub mod config;
#[cfg(not(target_arch = "wasm32"))]
pub mod logger;
#[cfg(not(target_arch = "wasm32"))]
pub mod mioserver;
#[cfg(not(target_arch = "wasm32"))]
pub mod stream;
#[cfg(not(target_arch = "wasm32"))]
pub mod voip;
#[cfg(not(target_arch = "wasm32"))]
pub mod udp;
#[cfg(not(target_arch = "wasm32"))]
pub mod utils;
#[cfg(not(target_arch = "wasm32"))]
pub mod client;

#[cfg(not(target_arch = "wasm32"))]
pub use client::api::{run_measurement, run_measurement_with_progress, MeasurementResult};
#[cfg(not(target_arch = "wasm32"))]
pub use client::client::{ClientConfig, SharedStats};
#[cfg(not(target_arch = "wasm32"))]
pub use client::live::{new_shared_live, LiveState, SharedLive};

// Reactor abstraction: the interface that decouples the RMBT state machine from
// its concrete readiness source (mio `Poll` natively, a JS-driven pump on wasm).
// Extracting `poll` behind this trait is what lets the same handler logic run in
// a browser over a WebSocket instead of a mio/epoll event loop.
pub mod reactor;

// Browser WASM entry point: the JS-driven WebSocket transport (`JsStream`) that
// substitutes for `mio::net::TcpStream`, plus the reactor implementation.
#[cfg(target_arch = "wasm32")]
pub mod wasm;
