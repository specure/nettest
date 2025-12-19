use lazy_static::lazy_static;
use std::sync::atomic::{AtomicU32, Ordering};

/// Default buffer size for read operations
pub const DEFAULT_READ_BUFFER_SIZE: usize = 1024 * 1024;

/// Default buffer size for write operations
pub const DEFAULT_WRITE_BUFFER_SIZE: usize = 1024;

/// HTTP upgrade request template for RMBT protocol
pub const RMBT_UPGRADE_REQUEST: &str = "GET /rmbt HTTP/1.1 \r\n\
    Connection: Upgrade \r\n\
    Upgrade: RMBT\r\n\
    RMBT-Version: 1.2.0\r\n\
    \r\n";

/// String that indicates token acceptance from server
pub const ACCEPT_TOKEN_STRING: &str = "ACCEPT TOKEN";

/// String that indicates acceptance of GETCHUNKS command and available commands
pub const ACCEPT_GETCHUNKS_STRING: &str = "ACCEPT GETCHUNKS GETTIME PUT PUTNORESULT PING QUIT\n";

/// GETCHUNKS command
pub const GETCHUNKS_COMMAND: &[u8] = b"GETCHUNKS\n";

/// Minimum chunk size (4KB)
pub const MIN_CHUNK_SIZE: u32 = 4096;

/// Default maximum chunk size (4MB)
const DEFAULT_MAX_CHUNK_SIZE: u32 = 4194304;

// Static variable for maximum chunk size (configurable via config file)
lazy_static! {
    pub static ref MAX_CHUNK_SIZE: AtomicU32 = AtomicU32::new(DEFAULT_MAX_CHUNK_SIZE);
}

/// Initialize MAX_CHUNK_SIZE from config
pub fn init_max_chunk_size(max_chunk_size: Option<u32>) {
    let value = max_chunk_size.unwrap_or(DEFAULT_MAX_CHUNK_SIZE);
    MAX_CHUNK_SIZE.store(value, Ordering::Relaxed);
}

/// Get current MAX_CHUNK_SIZE value
pub fn get_max_chunk_size() -> u32 {
    MAX_CHUNK_SIZE.load(Ordering::Relaxed)
}

/// Pre-download duration in nanoseconds (2 seconds)
pub const PRE_DOWNLOAD_DURATION_NS: u64 = 2_000_000_000;

/// Maximum number of chunks before increasing chunk size
pub const MAX_CHUNKS_BEFORE_SIZE_INCREASE: u32 = 8;

/// Time buffer size
pub const TIME_BUFFER_SIZE: usize = 1024;

/// OK command
pub const OK_COMMAND: &[u8] = b"OK\n";
