// Handlers needed for the browser (greeting/ping/pretest/download/upload/
// signed result) compile on both targets. Only the UDP-based jitter/packet-loss
// handlers stay native-only — a browser has no UDP socket.
pub mod greeting;
pub mod get_chunks;
pub mod ping;
pub mod basic_handler;
pub mod get_time;
pub mod put;
pub mod puttimeresult;
pub mod signed_result;

#[cfg(not(target_arch = "wasm32"))]
pub mod voip;
#[cfg(not(target_arch = "wasm32"))]
pub mod udp;
