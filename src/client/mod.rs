// Core pieces needed for the browser measurement (state machine, handlers,
// constants, calculator, live samples) compile on both targets.
pub mod constants;
pub mod globals;
pub mod handlers;
pub mod state;
pub mod calculator;
pub mod live;
pub mod user_agent;

// `client` (ClientConfig/SharedStats + the CLI/native orchestration) pulls the
// runner/api/print/config — native-only.
#[cfg(not(target_arch = "wasm32"))]
pub mod client;

// Native-only: plotting/printing, the threaded runner, the reqwest control-server
// client, the CLI arg parser, and the high-level api that ties them together.
#[cfg(not(target_arch = "wasm32"))]
pub mod print;
#[cfg(not(target_arch = "wasm32"))]
pub mod runnner;
#[cfg(not(target_arch = "wasm32"))]
pub mod args_parser;
#[cfg(not(target_arch = "wasm32"))]
pub mod control_server;
#[cfg(not(target_arch = "wasm32"))]
pub mod api;
