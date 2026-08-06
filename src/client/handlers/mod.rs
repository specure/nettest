// Handlers needed for the browser (greeting/ping/download) compile on both
// targets. Upload (put/puttimeresult), signed result, and the UDP-based
// jitter/packet-loss handlers are native-only for now.
pub mod greeting;
pub mod get_chunks;
pub mod ping;
pub mod basic_handler;
pub mod get_time;

#[cfg(not(target_arch = "wasm32"))]
pub mod put;
#[cfg(not(target_arch = "wasm32"))]
pub mod puttimeresult;
#[cfg(not(target_arch = "wasm32"))]
pub mod signed_result;
#[cfg(not(target_arch = "wasm32"))]
pub mod voip;
#[cfg(not(target_arch = "wasm32"))]
pub mod udp;
