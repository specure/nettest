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

/// Maximum chunk size (4MB)
pub const MAX_CHUNK_SIZE: u32 = 4194304;

/// Pre-download duration in nanoseconds (2 seconds)
pub const PRE_DOWNLOAD_DURATION_NS: u64 = 2_000_000_000;

/// Maximum number of chunks before increasing chunk size
pub const MAX_CHUNKS_BEFORE_SIZE_INCREASE: u32 = 8;

/// Time buffer size
pub const TIME_BUFFER_SIZE: usize = 1024;

/// OK command
pub const OK_COMMAND: &[u8] = b"OK\n";
