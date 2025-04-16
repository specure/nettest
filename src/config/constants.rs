pub const DEFAULT_NUM_THREADS: usize = 200;
pub const CHUNK_SIZE: usize = 4096; // 4 KiB
pub const MIN_CHUNK_SIZE: usize = 4096; // 4 KiB
pub const MAX_CHUNK_SIZE: usize = 4194304; // 4 MiB
pub const GREETING: &str = "RMBTv2\n";
pub const GREETING_V3: &str = "RMBTv3\n";
pub const ACCEPT_TOKEN: &str = "ACCEPT TOKEN QUIT\n";
pub const ACCEPT_COMMANDS: &str = "ACCEPT GETCHUNKS GETTIME PUT PUTNORESULT PING QUIT\n";
pub const TOKEN_PATTERN: &str = r"TOKEN ([a-zA-Z0-9-]+)_(\d+)_([a-zA-Z0-9+/=]+)";

// Command constants
pub const CMD_GETCHUNKS: &str = "GETCHUNKS";
pub const CMD_GETTIME: &str = "GETTIME";
pub const CMD_PUT: &str = "PUT";
pub const CMD_PUTNORESULT: &str = "PUTNORESULT";
pub const CMD_PING: &str = "PING";
pub const CMD_QUIT: &str = "QUIT";

// Response constants
pub const RESP_OK: &str = "OK\n";
pub const RESP_ERR: &str = "ERR\n";
pub const RESP_BYE: &str = "BYE\n";
pub const RESP_PONG: &str = "PONG\n";
pub const RESP_TIME: &str = "TIME";

// Size constants
pub const MAX_CHUNKS: usize = 1000;
pub const MAX_PUT_SIZE: usize = 1024 * 1024; // 1MB
pub const MAX_LINE_LENGTH: usize = 1024;
