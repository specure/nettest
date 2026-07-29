pub mod config;
pub mod logger;
// The server implementation is not needed by the mobile (Android/iOS) FFI
// client and pulls in mdns-sd/include_dir, which are excluded from the
// Android dependency set (see Cargo.toml) - keep it host/iOS/desktop-only.
#[cfg(not(target_os = "android"))]
pub mod mioserver;
pub mod stream;
pub mod voip;
pub mod udp;
pub mod utils;
pub mod client;

pub use client::api::{run_measurement, run_measurement_with_progress, MeasurementResult};
pub use client::client::{ClientConfig, SharedStats};
pub use client::live::{new_shared_live, LiveState, SharedLive};
