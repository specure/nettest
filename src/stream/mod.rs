// `stream` (the Stream transport interface) compiles on both targets; the
// concrete variants differ by target. Native TLS/WebSocket transports are
// native-only; the browser gets `js_wss`.
pub mod stream;

#[cfg(not(target_arch = "wasm32"))]
pub mod websocket;
#[cfg(not(target_arch = "wasm32"))]
pub mod websocket_tls_openssl;
#[cfg(not(target_arch = "wasm32"))]
pub mod rustls_server;
#[cfg(not(target_arch = "wasm32"))]
pub mod rustls;
#[cfg(not(target_arch = "wasm32"))]
pub mod openssl;
#[cfg(not(target_arch = "wasm32"))]
pub mod websocket_rustls_server;

#[cfg(target_arch = "wasm32")]
pub mod js_wss;
#[cfg(target_arch = "wasm32")]
pub mod wt_datagrams;
