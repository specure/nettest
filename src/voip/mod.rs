pub mod calculator;
pub mod rtp;
pub mod udp;

pub use calculator::RtpQoSResult;

pub const DEFAULT_VOIP_UDP_PORT: u16 = 5004;
pub const DEFAULT_SAMPLE_RATE: u32 = 8000;
pub const DEFAULT_BITS_PER_SAMPLE: u8 = 8;
pub const DEFAULT_DELAY_MS: u64 = 20;
pub const DEFAULT_DURATION_MS: u64 = 1000;
pub const DEFAULT_PAYLOAD_TYPE: u8 = 8; // PCMA
pub const DEFAULT_BUFFER_NS: u64 = 100_000_000;

#[derive(Debug, Clone)]
pub struct VoipParams {
    pub out_port: u16,
    pub in_port: u16,
    pub sample_rate: u32,
    pub bits_per_sample: u8,
    pub delay_ms: u64,
    pub duration_ms: u64,
    pub initial_seq: u16,
    pub payload_type: u8,
    pub buffer_ns: u64,
}

impl VoipParams {
    pub fn num_packets(&self) -> u64 {
        self.duration_ms / self.delay_ms
    }

    pub fn payload_size(&self) -> usize {
        (self.sample_rate as u64 * self.delay_ms / 1000 * self.bits_per_sample as u64 / 8) as usize
    }

    pub fn timestamp_increment(&self) -> u32 {
        (self.sample_rate as u64 * self.delay_ms / 1000) as u32
    }

    pub fn to_command_args(&self) -> String {
        format!(
            "{} {} {} {} {} {} {} {} {}",
            self.out_port,
            self.in_port,
            self.sample_rate,
            self.bits_per_sample,
            self.delay_ms,
            self.duration_ms,
            self.initial_seq,
            self.payload_type,
            self.buffer_ns,
        )
    }

    pub fn from_command_args(args: &str) -> Option<Self> {
        let parts: Vec<&str> = args.trim().split_whitespace().collect();
        if parts.len() < 9 {
            return None;
        }
        Some(Self {
            out_port:        parts[0].parse().ok()?,
            in_port:         parts[1].parse().ok()?,
            sample_rate:     parts[2].parse().ok()?,
            bits_per_sample: parts[3].parse().ok()?,
            delay_ms:        parts[4].parse().ok()?,
            duration_ms:     parts[5].parse().ok()?,
            initial_seq:     parts[6].parse().ok()?,
            payload_type:    parts[7].parse().ok()?,
            buffer_ns:       parts[8].parse().ok()?,
        })
    }
}
