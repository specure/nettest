// The wire format and the statistics are plain arithmetic and compile
// everywhere, so the browser (wasm) client can reuse them over WebTransport
// datagrams and produce results comparable to the native UDP test. Only the
// socket plumbing below is native-only.
pub mod payload;
pub mod result;
#[cfg(not(target_arch = "wasm32"))]
pub mod server;
#[cfg(not(target_arch = "wasm32"))]
pub mod socket;

pub use result::UdpQoSResult;
#[cfg(not(target_arch = "wasm32"))]
pub use server::SharedUdpServer;

pub const DEFAULT_UDP_OUT_NUM_PACKETS: u32 = 10;
pub const DEFAULT_UDP_IN_NUM_PACKETS:  u32 = 10;
pub const DEFAULT_UDP_DELAY_NS:        u64 = 200_000_000; // 200 ms
pub const DEFAULT_UDP_TMAX_NS:         u64 = 1_000_000_000; // 1 s late-packet window per direction (OUT+IN -> ~2 s total)
pub const DEFAULT_UDP_SERVER_PORT:     u16 = 5004; // same as VoIP — tests run sequentially
