use std::time::Duration;

pub const DEFAULT_PORT: u16 = 8080;
pub const DEFAULT_VERSION: u32 = 2;
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);
pub const DEFAULT_TOKEN_TTL: Duration = Duration::from_secs(60);
pub const DEFAULT_TOKEN_LABEL: &str = "test_label";
pub const DEFAULT_TOKEN_KEY: &str = "test_key"; 