use std::collections::{BTreeMap, HashMap};

#[derive(Debug, Clone, PartialEq)]
pub enum PacketOutcome {
    Received,
    Lost,
    Undefined,
}

#[derive(Debug, Clone)]
pub struct PacketRecord {
    pub packet_number: u32,
    pub send_time_ns:  u64,
    pub deadline_ns:   u64,
    pub return_time:   Option<u64>,
}

#[derive(Debug, Clone, Default)]
pub struct UdpQoSResult {
    pub sent_packets:      usize,
    pub received_packets:  usize,
    pub lost_packets:      usize,
    pub undefined_packets: usize,
    pub duplicate_packets: usize,
    pub packet_loss_rate:  i32,
    pub max_burst_loss:    u32,
    pub loss_episodes:     u32,
    pub rtt_avg_ns:        Option<u64>,
    pub rtt_min_ns:        Option<u64>,
    pub rtt_max_ns:        Option<u64>,
    pub rtts_ns:           BTreeMap<u32, u64>,
}

fn classify(record: &PacketRecord) -> PacketOutcome {
    match record.return_time {
        Some(t) if t <= record.deadline_ns => PacketOutcome::Received,
        Some(_)                            => PacketOutcome::Undefined,
        None                               => PacketOutcome::Lost,
    }
}

pub fn calculate_qos(
    records: &HashMap<u32, PacketRecord>,
    sent_count: usize,
    duplicate_count: usize,
) -> UdpQoSResult {
    let mut by_seq: Vec<&PacketRecord> = records.values().collect();
    by_seq.sort_by_key(|r| r.packet_number);

    let sample: Vec<PacketOutcome> = by_seq.iter().map(|r| classify(r)).collect();

    let received  = sample.iter().filter(|o| **o == PacketOutcome::Received).count();
    let undefined = sample.iter().filter(|o| **o == PacketOutcome::Undefined).count();
    let lost      = sample.iter().filter(|o| **o == PacketOutcome::Lost).count();

    let deterministic = sent_count.saturating_sub(undefined);
    let packet_loss_rate = if deterministic == 0 || received >= deterministic {
        0i32
    } else {
        ((deterministic - received) as f32 / deterministic as f32 * 100.0) as i32
    };

    // RFC 6673 §5 burst detection — Undefined breaks a run
    let mut max_burst:     u32 = 0;
    let mut cur_burst:     u32 = 0;
    let mut loss_episodes: u32 = 0;
    let mut prev_loss            = false;

    for outcome in &sample {
        match outcome {
            PacketOutcome::Lost => {
                cur_burst += 1;
                if !prev_loss { loss_episodes += 1; }
                prev_loss = true;
            }
            _ => {
                max_burst = max_burst.max(cur_burst);
                cur_burst = 0;
                prev_loss = false;
            }
        }
    }
    max_burst = max_burst.max(cur_burst);

    // RTT — only for Received packets (echo within Tmax)
    let rtts_ns: BTreeMap<u32, u64> = records
        .values()
        .filter_map(|r| {
            r.return_time.and_then(|t| {
                if t <= r.deadline_ns {
                    Some((r.packet_number, t - r.send_time_ns))
                } else {
                    None
                }
            })
        })
        .collect();

    let rtt_avg_ns = if rtts_ns.is_empty() {
        None
    } else {
        Some(rtts_ns.values().sum::<u64>() / rtts_ns.len() as u64)
    };
    let rtt_min_ns = rtts_ns.values().copied().min();
    let rtt_max_ns = rtts_ns.values().copied().max();

    UdpQoSResult {
        sent_packets:      sent_count,
        received_packets:  received,
        lost_packets:      lost,
        undefined_packets: undefined,
        duplicate_packets: duplicate_count,
        packet_loss_rate,
        max_burst_loss:    max_burst,
        loss_episodes,
        rtt_avg_ns,
        rtt_min_ns,
        rtt_max_ns,
        rtts_ns,
    }
}

pub fn packet_loss_rate_simple(sent: usize, received: usize) -> i32 {
    if received >= sent {
        return 0;
    }
    ((sent - received) as f32 / sent as f32 * 100.0) as i32
}

// Server-side result types
#[derive(Debug, Clone)]
pub struct UdpServerOutResult {
    pub received: u32,
    pub port:     u16,
}

#[derive(Debug, Clone)]
pub struct UdpServerInResult {
    pub received: u32,
    pub port:     u16,
    pub rtts:     BTreeMap<u32, u64>,
}
