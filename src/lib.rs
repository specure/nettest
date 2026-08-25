// Native-only modules: raw sockets (mio), TLS, threads, the mio server, UDP
// (jitter/packet-loss), plotting/printing, HTTP control-server client. None of
// these compile for a browser WASM target.
#[cfg(not(target_arch = "wasm32"))]
pub mod config;
#[cfg(not(target_arch = "wasm32"))]
pub mod logger;
#[cfg(not(target_arch = "wasm32"))]
pub mod mioserver;
// `stream` compiles on both targets (Stream is cfg-split: native transports vs
// the browser JsWss variant).
pub mod stream;
// Browser QoS over QUIC datagrams: the server side of the WebTransport jitter /
// packet-loss test. Native-only — it *is* the server.
#[cfg(not(target_arch = "wasm32"))]
pub mod wtqos;

// `voip` and `udp` are cfg-split inside: the QoS statistics and wire formats
// compile on both targets, the socket drivers are native-only.
pub mod voip;
pub mod udp;
#[cfg(not(target_arch = "wasm32"))]
pub mod utils;
// `client` (state machine + handlers) compiles on both targets; native-only
// submodules (runner/api/control_server/print/udp-voip handlers) are gated
// inside client/*.
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
