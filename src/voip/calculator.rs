use crate::voip::rtp::PacketMap;

#[derive(Debug, Clone, Default)]
pub struct RtpQoSResult {
    pub received_packets: usize,
    pub max_jitter:        i64,
    pub mean_jitter:       i64,
    pub skew:              i64,
    pub max_delta:         i64,
    pub out_of_order:      i32,
    pub min_sequential:    i32,
    pub max_sequential:    i32,
    pub number_of_stalls:  i32,
    pub avg_stall_time:    i64,
}

impl RtpQoSResult {
    pub fn to_voip_result_string(&self) -> String {
        format!(
            "VOIPRESULT {} {} {} {} {} {} {} {} {} {}",
            self.max_jitter,
            self.mean_jitter,
            self.max_delta,
            self.skew,
            self.received_packets,
            self.out_of_order,
            self.min_sequential,
            self.max_sequential,
            self.number_of_stalls,
            self.avg_stall_time,
        )
    }

    pub fn from_voip_result_string(s: &str) -> Option<Self> {
        let s = s.trim().strip_prefix("VOIPRESULT ")?;
        let parts: Vec<&str> = s.split_whitespace().collect();
        if parts.len() < 10 {
            return None;
        }
        Some(Self {
            max_jitter:       parts[0].parse().ok()?,
            mean_jitter:      parts[1].parse().ok()?,
            max_delta:        parts[2].parse().ok()?,
            skew:             parts[3].parse().ok()?,
            received_packets: parts[4].parse().ok()?,
            out_of_order:     parts[5].parse().ok()?,
            min_sequential:   parts[6].parse().ok()?,
            max_sequential:   parts[7].parse().ok()?,
            number_of_stalls: parts[8].parse().ok()?,
            avg_stall_time:   parts[9].parse().ok()?,
        })
    }
}

/// Jitter calculation per RFC 3550 Appendix A.8.
/// See Nettest_Voip_Jitter_EN.md for algorithm details.
pub fn calculate_qos(
    packets:     &PacketMap,
    initial_seq: u16,
    sample_rate: u32,
    buffer_ns:   u64,
) -> RtpQoSResult {
    if packets.is_empty() {
        return RtpQoSResult::default();
    }

    // --- Pass 1: jitter, skew, stalls (sorted by sequence_number) ---
    let mut by_seq: Vec<_> = packets.values().collect();
    by_seq.sort_by_key(|p| p.sequence_number);

    let mut jitter: f32 = 0.0;
    let mut max_jitter:  i64 = 0;
    let mut mean_jitter: i64 = 0;
    let mut max_delta:   i64 = 0;
    let mut skew:        i64 = 0;
    let mut stalls:      i32 = 0;
    let mut stall_time:  i64 = 0;

    let mut prev: Option<&crate::voip::rtp::RtpControlData> = None;
    for cur in &by_seq {
        match prev {
            None => {
                // first packet: running estimate starts at zero
                jitter = 0.0;
            }
            Some(i) => {
                let real_diff_ns = cur.received_ns as i64 - i.received_ns as i64;

                if real_diff_ns > buffer_ns as i64 {
                    stalls += 1;
                    stall_time += real_diff_ns - buffer_ns as i64;
                }

                // Truncate to whole ms before converting to ns — matches RTP timer resolution
                let ts_diff_ms = (cur.rtp_timestamp.wrapping_sub(i.rtp_timestamp) as f32
                    / sample_rate as f32
                    * 1000.0) as i64;
                let expected_diff_ns = ts_diff_ms * 1_000_000i64;

                let delta = (real_diff_ns - expected_diff_ns).unsigned_abs() as i64;

                // RFC 3550: J(i) = J(i-1) + (|D(i-1,i)| - J(i-1)) / 16
                jitter += (delta as f32 - jitter) / 16.0;

                // truncation via `as i64`, not rounding
                max_jitter  = max_jitter.max(jitter as i64);
                mean_jitter += jitter as i64;
                max_delta   = max_delta.max(delta);
                skew        += expected_diff_ns - real_diff_ns;
            }
        }
        prev = Some(cur);
    }

    // Divide by n (total packets), not n-1 (pairs).
    // The first packet contributes 0.0 to the sum but is included in the count.
    let n = by_seq.len() as i64;
    if n > 0 {
        mean_jitter /= n;
    }

    let avg_stall_time: i64 = if stalls > 0 {
        stall_time / 1_000_000 / stalls as i64  // ns → ms, then average
    } else {
        0
    };

    // --- Pass 2: out-of-order analysis (sorted by received_ns) ---
    let mut by_time: Vec<_> = packets.values().collect();
    by_time.sort_by_key(|p| p.received_ns);

    let mut next_expected:  u16 = initial_seq;
    let mut out_of_order:   i32 = 0;
    let mut cur_sequential: i32 = 0;
    let mut max_sequential: i32 = 0;
    let mut min_sequential: i32 = 0; // 0 = not yet observed

    for pkt in &by_time {
        if pkt.sequence_number != next_expected {
            out_of_order += 1;
            max_sequential = max_sequential.max(cur_sequential);
            if cur_sequential > 1 {
                min_sequential = update_min(cur_sequential, min_sequential);
            }
            cur_sequential = 0;
        } else {
            cur_sequential += 1;
        }
        next_expected = next_expected.wrapping_add(1);
    }

    max_sequential = max_sequential.max(cur_sequential);
    if cur_sequential > 1 {
        min_sequential = update_min(cur_sequential, min_sequential);
    }
    if min_sequential == 0 && max_sequential > 0 {
        min_sequential = max_sequential;
    }

    // Cap sequential counts at total received
    let received = packets.len() as i32;
    RtpQoSResult {
        received_packets: packets.len(),
        max_jitter,
        mean_jitter,
        skew,
        max_delta,
        out_of_order,
        min_sequential: if min_sequential > received { received } else { min_sequential },
        max_sequential: if max_sequential > received { received } else { max_sequential },
        number_of_stalls: stalls,
        avg_stall_time,
    }
}

fn update_min(cur: i32, min: i32) -> i32 {
    if cur < min { cur } else if min == 0 { cur } else { min }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::voip::rtp::RtpControlData;
    use std::collections::HashMap;

    #[test]
    fn test_perfect_stream_zero_jitter() {
        let sample_rate = 8000u32;
        let delay_ms = 20u64;
        let ts_increment = (sample_rate as u64 * delay_ms / 1000) as u32; // 160

        let mut packets = HashMap::new();
        for i in 0u16..10 {
            packets.insert(i, RtpControlData {
                sequence_number: i,
                rtp_timestamp: (i as u32) * ts_increment,
                received_ns: (i as u64) * delay_ms * 1_000_000,
            });
        }

        let result = calculate_qos(&packets, 0, sample_rate, 100_000_000);
        assert_eq!(result.received_packets, 10);
        assert_eq!(result.max_jitter, 0);
        assert_eq!(result.mean_jitter, 0);
        assert_eq!(result.out_of_order, 0);
        assert_eq!(result.number_of_stalls, 0);
    }

    #[test]
    fn test_mean_jitter_divides_by_total_packets_not_pairs() {
        // mean_jitter = sum of jitter estimates / n (total packets), not / n-1 (pairs)
        let sample_rate = 8000u32;
        let ts_increment = 160u32;

        let mut packets = HashMap::new();
        packets.insert(0u16, RtpControlData { sequence_number: 0, rtp_timestamp: 0, received_ns: 0 });
        // packet 1 arrives 5ms late
        packets.insert(1u16, RtpControlData { sequence_number: 1, rtp_timestamp: ts_increment, received_ns: 25_000_000 });
        packets.insert(2u16, RtpControlData { sequence_number: 2, rtp_timestamp: ts_increment * 2, received_ns: 40_000_000 });

        let result = calculate_qos(&packets, 0, sample_rate, 100_000_000);
        assert!(result.mean_jitter < result.max_jitter, "mean must be < max when divided by n");
    }

    #[test]
    fn test_update_min() {
        assert_eq!(update_min(3, 0), 3);  // min not yet observed → take cur
        assert_eq!(update_min(3, 5), 3);  // cur < min → take cur
        assert_eq!(update_min(5, 3), 3);  // cur > min → keep min
        assert_eq!(update_min(3, 3), 3);  // equal → keep min
    }
}
