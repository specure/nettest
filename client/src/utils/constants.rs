/// Default buffer size for read operations
pub const DEFAULT_READ_BUFFER_SIZE: usize = 1024;

/// Default buffer size for write operations
pub const DEFAULT_WRITE_BUFFER_SIZE: usize = 1024;

/// HTTP upgrade request template for RMBT protocol
pub const RMBT_UPGRADE_REQUEST: &str = "GET /rmbt HTTP/1.1 \r\n\
    Connection: Upgrade \r\n\
    Upgrade: RMBT\r\n\
    RMBT-Version: 1.2.0\r\n\
    \r\n";
