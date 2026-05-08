pub mod payload;
pub mod result;
pub mod server;
pub mod socket;

pub use result::UdpQoSResult;
pub use server::SharedUdpServer;

pub const DEFAULT_UDP_OUT_NUM_PACKETS: u32 = 10;
pub const DEFAULT_UDP_IN_NUM_PACKETS:  u32 = 10;
pub const DEFAULT_UDP_DELAY_NS:        u64 = 200_000_000; // 200 ms
pub const DEFAULT_UDP_TMAX_NS:         u64 = 3_000_000_000; // 3 s
pub const DEFAULT_UDP_SERVER_PORT:     u16 = 5004; // same as VoIP — tests run sequentially
