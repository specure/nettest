use std::collections::HashMap;
use std::time::Instant;

pub const RTP_HEADER_SIZE: usize = 12;

thread_local! {
    static EPOCH: Instant = Instant::now();
}

pub fn now_ns() -> u64 {
    EPOCH.with(|e| e.elapsed().as_nanos() as u64)
}

#[derive(Debug, Clone)]
pub struct RtpPacket {
    pub sequence_number: u16,
    pub timestamp: u32,
    pub ssrc: u32,
    pub payload_type: u8,
    pub marker: bool,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct RtpControlData {
    pub sequence_number: u16,
    pub rtp_timestamp: u32,
    pub received_ns: u64,
}

impl RtpPacket {
    pub fn new(
        seq: u16,
        ts: u32,
        ssrc: u32,
        payload_type: u8,
        marker: bool,
        payload_size: usize,
    ) -> Self {
        let payload: Vec<u8> = (0..payload_size).map(|i| ((i ^ seq as usize) & 0xff) as u8).collect();
        Self { sequence_number: seq, timestamp: ts, ssrc, payload_type, marker, payload }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = vec![0u8; RTP_HEADER_SIZE + self.payload.len()];
        buf[0] = 2 << 6; // V=2, P=0, X=0, CC=0
        buf[1] = ((self.marker as u8) << 7) | (self.payload_type & 0x7f);
        buf[2..4].copy_from_slice(&self.sequence_number.to_be_bytes());
        buf[4..8].copy_from_slice(&self.timestamp.to_be_bytes());
        buf[8..12].copy_from_slice(&self.ssrc.to_be_bytes());
        buf[RTP_HEADER_SIZE..].copy_from_slice(&self.payload);
        buf
    }

    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < RTP_HEADER_SIZE {
            return None;
        }
        Some(Self {
            marker: (data[1] & 0x80) != 0,
            payload_type: data[1] & 0x7f,
            sequence_number: u16::from_be_bytes([data[2], data[3]]),
            timestamp: u32::from_be_bytes([data[4], data[5], data[6], data[7]]),
            ssrc: u32::from_be_bytes([data[8], data[9], data[10], data[11]]),
            payload: data[RTP_HEADER_SIZE..].to_vec(),
        })
    }
}

pub type PacketMap = HashMap<u16, RtpControlData>;
