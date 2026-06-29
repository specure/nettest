# UDP Packet Loss Measurement — nettest Implementation

This document describes how UDP packet loss and RTT are measured in the nettest client
using the RFC 6673 round-trip packet loss algorithm (`src/udp/`).

See also: [Nettest_Voip_Jitter_EN.md](Nettest_Voip_Jitter_EN.md)

---

## Background: Standards for Packet Loss Measurement

Several IETF standards define how packet loss should be measured. Each targets a different
use case and level of rigour.

| Standard | Full name | Approach |
|---|---|---|
| **RFC 2330** | Framework for IP Performance Metrics (IPPM) | General framework; defines singleton, sample, and statistic metrics |
| **RFC 2680** | A One-way Packet Loss Metric for IPPM | One-way loss; requires synchronised clocks at both endpoints |
| **RFC 6673** | Round-Trip Packet Loss Metrics | Bidirectional loss via echo; no clock synchronisation required |
| **RFC 4656** | One-Way Active Measurement Protocol (OWAMP) | Protocol implementing RFC 2680; needs NTP/PTP infrastructure |
| **RFC 5357** | Two-Way Active Measurement Protocol (TWAMP) | Operator-grade bidirectional measurement; needs TWAMP reflector |

### RFC 2680 — One-way Packet Loss

The foundational IETF definition of packet loss. A packet sent at time `Ts` from `Src` to
`Dst` is either received (`0`) or lost (`1`). Because the measurement is one-way, it
requires both endpoints to have synchronised clocks accurate to sub-millisecond precision
(NTP stratum 1 or PTP). Consumer devices cannot guarantee this accuracy, making absolute
one-way loss values unreliable in practice.

### RFC 6673 — Round-Trip Packet Loss

Extends the RFC 2680 framework to round-trip measurements. The source sends a packet and
waits for the far end to echo it back. Loss is measured entirely on the source's own
monotonic clock, so no clock synchronisation is required. Three outcomes are defined:

- **0 (Received)** — echo returned within threshold `Tmax`
- **1 (Lost)** — echo did not return within `Tmax`
- **undefined** — echo returned after `Tmax` but before the measurement window closed;
  the packet cannot be classified as lost or received

The loss ratio excludes `undefined` packets from both numerator and denominator. Burst
metrics (consecutive loss runs) are defined in Section 5.

### RFC 4656 — OWAMP

A complete active measurement protocol implementing RFC 2680 metrics. Requires an
OWAMP server with access to a high-precision time source. Impractical for consumer
devices.

### RFC 5357 — TWAMP

Bidirectional active measurement designed for ISP infrastructure. Requires a TWAMP
reflector on the far end and a binary control session. Not suitable for a measurement
server that needs to interoperate with browser and mobile clients over a standard TCP
control connection.

### Why RFC 6673 was chosen

RFC 6673 is the right fit for nettest for three reasons:

1. **Works without clock synchronisation.** Loss is measured on the sender's monotonic
   clock only. The `Tmax` threshold (`timeout` parameter) removes the ambiguity of late
   arrivals without requiring the receiver's clock.

2. **Reuses the existing control channel.** The test protocol shares the same TCP
   connection used for download and upload phases. No additional server daemon or
   reflector is required.

3. **Matches the VoIP jitter model.** The `AWAIT_RESPONSE` / `RESPONSE` echo mechanism
   mirrors the bidirectional RTP stream used by the VoIP test. Both directions (client →
   server and server → client) are tested sequentially on the same connection, producing
   directional loss rates and RTT statistics in a single test run.

The trade-off is that `undefined` packets introduce a small amount of ambiguity at high
latency. For a consumer broadband test this is acceptable — a connection where echoes
regularly arrive after 3 seconds would already be flagged as poor quality.

---

## 1. Parameters and Default Values

| Parameter | Config key | Default | Description |
|---|---|---|---|
| `out_num_packets` | `out_num_packets` | `10` | Packet count client→server |
| `in_num_packets` | `in_num_packets` | `10` | Packet count server→client |
| `out_port` | `out_port` | via `GET UDPPORT` | Server UDP port for outgoing stream |
| `in_port` | `in_port` | `0` → dynamic | Client UDP port for incoming stream |
| `delay` | `delay` | `200_000_000` ns (200 ms) | Inter-packet interval |
| `tmax` | `timeout` | `3_000_000_000` ns (3 s) | Per-packet echo timeout (RFC 6673 threshold) |

---

## 2. Protocol

### Outgoing test (client → server)

1. `GET UDPPORT` → server responds with `<port>`
2. `UDPTEST OUT <port> <packet_count>` → server responds `OK`
3. Client sends `packet_count` UDP packets to `<port>`; server echoes each back
4. `GET UDPRESULT OUT <port>` → server responds `RCV <received_count> <port>`

### Incoming test (server → client)

1. Client binds a local UDP socket on `in_port`
2. `UDPTEST IN <in_port> <packet_count>` → server sends `packet_count` packets to `in_port`
3. Client echoes each received packet back to the server
4. `GET UDPRESULT IN <in_port>` → server responds `RCV <received_count> <port> <json_rtts>`

The two sub-tests run sequentially on the same TCP control connection, separated by
the `GET UDPRESULT` query of the first sub-test.

---

## 3. Packet Structure

Each UDP datagram carries a `UdpPayload`:

| Field | Offset | Type | Description |
|---|---|---|---|
| `communication_flag` | 0 | u8 | `1` = ONE_DIRECTION, `2` = RESPONSE, `3` = AWAIT_RESPONSE |
| `packet_number` | 1 | u32 (big-endian) | Sequence number, 0-based |
| `uuid` | 5 | [u8; 16] | Client identifier (UUID v4, raw bytes) |
| `timestamp_ns` | 21 | i64 (big-endian) | Monotonic nanoseconds at send time |

Total payload: 29 bytes.

The client sends with `AWAIT_RESPONSE (3)`; the server echoes back the identical payload
with `communication_flag` replaced by `RESPONSE (2)`. The `timestamp_ns` field is
preserved in the echo, allowing the client to compute RTT as `now_ns() − timestamp_ns`.

Serialisation (`src/udp/payload.rs`):

```rust
pub fn to_bytes(&self) -> [u8; UDP_PAYLOAD_SIZE] {
    let mut buf = [0u8; UDP_PAYLOAD_SIZE];
    buf[0] = self.communication_flag;
    buf[1..5].copy_from_slice(&self.packet_number.to_be_bytes());
    buf[5..21].copy_from_slice(&self.uuid);
    buf[21..29].copy_from_slice(&self.timestamp_ns.to_be_bytes());
    buf
}

pub fn from_bytes(data: &[u8]) -> Option<Self> {
    if data.len() < UDP_PAYLOAD_SIZE { return None; }
    Some(Self {
        communication_flag: data[0],
        packet_number:      u32::from_be_bytes([data[1], data[2], data[3], data[4]]),
        uuid:               data[5..21].try_into().ok()?,
        timestamp_ns:       i64::from_be_bytes([
            data[21], data[22], data[23], data[24],
            data[25], data[26], data[27], data[28],
        ]),
    })
}
```

---

## 4. Packet Tracking

For each sent packet the client stores a `PacketRecord` keyed by `packet_number`
(`src/udp/result.rs`):

```rust
pub struct PacketRecord {
    pub packet_number: u32,
    pub send_time_ns:  u64,   // monotonic ns at send time
    pub deadline_ns:   u64,   // send_time_ns + tmax_ns
    pub return_time:   Option<u64>,  // monotonic ns when echo arrived
}

pub type PacketMap = HashMap<u32, PacketRecord>;
```

On each incoming echo:

```rust
let return_time = now_ns();
let payload = UdpPayload::from_bytes(&buf)?;

if payload.communication_flag != FLAG_RESPONSE { return; }

match records.get_mut(&payload.packet_number) {
    Some(rec) if rec.return_time.is_none() => {
        rec.return_time = Some(return_time);
    }
    Some(_) => {
        duplicate_packets.insert(payload.packet_number);
    }
    None => { /* unsolicited packet */ }
}
```

Duplicate echoes are tracked separately and do not update `return_time` — only the first
arrival is recorded.

---

## 5. RFC 6673 Packet Loss Algorithm

Implemented in `src/udp/result.rs` as `calculate_qos()`.

### Step 1. Classify each packet outcome

After the measurement window closes, each `PacketRecord` is classified:

```rust
pub enum PacketOutcome {
    Received,   // echo returned at or before deadline_ns
    Lost,       // no echo arrived
    Undefined,  // echo arrived after deadline_ns (late arrival)
}

let measurement_end = now_ns();

let outcome = match record.return_time {
    Some(t) if t <= record.deadline_ns => PacketOutcome::Received,
    Some(_)                            => PacketOutcome::Undefined,
    None                               => PacketOutcome::Lost,
};
```

A packet is `Undefined` only when its echo arrives after `Tmax` but before the socket
closes. This distinguishes a genuinely late packet from one that was never received at
all.

### Step 2. Build the ordered sample

RFC 6673 §4 defines `Type-P-Round-trip-Packet-Loss-Sample` as an ordered sequence of
`(send_time, outcome)` pairs sorted by sequence number:

```rust
let mut sample: Vec<(u64, PacketOutcome)> = records
    .values()
    .sorted_by_key(|r| r.packet_number)
    .map(|r| (r.send_time_ns, classify(r, &duplicate_packets)))
    .collect();
```

### Step 3. Compute the loss ratio (RFC 6673 §4.2)

`Undefined` packets are excluded from both numerator and denominator:

```rust
let sent        = sample.len();
let received    = sample.iter().filter(|(_, o)| matches!(o, Received)).count();
let undefined   = sample.iter().filter(|(_, o)| matches!(o, Undefined)).count();
let deterministic = sent - undefined;

let packet_loss_rate: i32 = if deterministic == 0 {
    0
} else if received >= deterministic {
    0
} else {
    ((deterministic - received) as f32 / deterministic as f32 * 100.0) as i32
};
```

Result key: `udp_result_out_packet_loss_rate` / `udp_result_in_packet_loss_rate` —
integer 0–100 (percent).

### Step 4. Burst detection (RFC 6673 §5)

A second pass over the ordered sample detects consecutive loss runs:

```rust
let mut max_burst:     u32 = 0;
let mut cur_burst:     u32 = 0;
let mut loss_episodes: u32 = 0;
let mut prev_loss            = false;

for (_, outcome) in &sample {
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
```

`Undefined` packets break a burst run — they are treated as non-loss events for the
purpose of burst counting.

### Step 5. RTT statistics

RTT is computed only for packets classified as `Received` (echo arrived within `Tmax`):

```rust
let rtts_ns: BTreeMap<u32, u64> = records
    .values()
    .filter_map(|r| {
        if let Some(t) = r.return_time {
            if t <= r.deadline_ns {
                return Some((r.packet_number, t - r.send_time_ns));
            }
        }
        None
    })
    .collect();

let rtt_avg_ns = if rtts_ns.is_empty() {
    None
} else {
    Some(rtts_ns.values().sum::<u64>() / rtts_ns.len() as u64)
};

let rtt_min_ns = rtts_ns.values().copied().min();
let rtt_max_ns = rtts_ns.values().copied().max();
```

Late echoes (`Undefined`) are excluded from RTT statistics because their transit time
exceeds the threshold and would skew the average.

### Result structure

```rust
// src/udp/result.rs
pub struct UdpQoSResult {
    pub sent_packets:      usize,
    pub received_packets:  usize,
    pub lost_packets:      usize,
    pub undefined_packets: usize,
    pub duplicate_packets: usize,
    pub packet_loss_rate:  i32,         // 0–100, excludes undefined
    pub max_burst_loss:    u32,         // longest consecutive loss run
    pub loss_episodes:     u32,         // number of distinct loss events
    pub rtt_avg_ns:        Option<u64>,
    pub rtt_min_ns:        Option<u64>,
    pub rtt_max_ns:        Option<u64>,
    pub rtts_ns:           BTreeMap<u32, u64>,  // packet_number → rtt_ns
    pub sample:            Vec<(u64, PacketOutcome)>,  // ordered (send_time_ns, outcome)
}
```

Result fields are reported with directional prefixes:
- Outgoing stream (client → server): `udp_result_out_<metric>`
- Incoming stream (server → client): `udp_result_in_<metric>`

---

## 6. RTT Metrics

| Result key | Description |
|---|---|
| `udp_result_out_rtt_avg_ns` | Average RTT, outgoing stream, ns |
| `udp_result_out_rtt_min_ns` | Minimum RTT, outgoing stream, ns |
| `udp_result_out_rtt_max_ns` | Maximum RTT, outgoing stream, ns |
| `udp_result_out_rtts_ns` | Per-packet RTTs: `BTreeMap<u32, u64>` |
| `udp_result_in_rtt_avg_ns` | Average RTT, incoming stream, ns |
| `udp_result_in_rtt_min_ns` | Minimum RTT, incoming stream, ns |
| `udp_result_in_rtt_max_ns` | Maximum RTT, incoming stream, ns |
| `udp_result_in_rtts_ns` | Per-packet RTTs: `BTreeMap<u32, u64>` |

RTT for the outgoing stream is measured at the client (client sent, server echoed).
RTT for the incoming stream is measured at the server (server sent, client echoed) and
returned in the `GET UDPRESULT IN` response as a JSON object `{"seq": rtt_ns}`.

---

## 7. Units of Measurement

| Metric | Internal unit | Output unit |
|---|---|---|
| `packet_loss_rate` | % (i32, 0–100) | % |
| `max_burst_loss` | packets (u32) | count |
| `loss_episodes` | count (u32) | count |
| RTT | ns (u64) | ns |
| `delay` | ns internally, ms in server command | — |
| `tmax` / `timeout` | ns internally | — |

---

## 8. Implementation Notes

### 8.1 Per-packet `Tmax` vs global socket timeout

The critical difference from a naïve implementation is that each packet carries its own
deadline (`send_time_ns + tmax_ns`). A global socket timeout would classify all
unacknowledged packets as lost at the same moment, making late-arriving echoes
indistinguishable from dropped packets. Per-packet deadlines allow echoes that arrive
after their individual `Tmax` to be classified as `Undefined` rather than `Lost`.

### 8.2 Three-valued outcome

The `PacketOutcome` enum has three variants, not two. Code that computes the loss ratio
must subtract `undefined_packets` from the denominator. Using `sent_packets` directly as
the denominator would inflate the loss rate on high-latency connections.

### 8.3 Outgoing direction: RMBT compatibility

For the outgoing sub-test the server reports `RCV <received_count>`, not individual
per-packet outcomes. The client cannot classify outgoing packets as `Undefined` — it
only knows the total count the server received. The outgoing loss rate therefore uses the
simplified formula:

```rust
// Outgoing: server count-based (no per-packet Tmax on server side)
let lost = sent_count.saturating_sub(server_received);
let packet_loss_rate = if lost == 0 { 0i32 }
    else { (lost as f32 / sent_count as f32 * 100.0) as i32 };
```

Full RFC 6673 classification (with `Undefined`) applies to the **incoming** direction
only, where the client tracks per-packet records with individual deadlines.

### 8.4 Duplicate echo handling

An echo with a `packet_number` already in `return_time` is a duplicate. Duplicates:
- are counted in `duplicate_packets`
- do not update `return_time` — only the first echo counts
- do not affect `packet_loss_rate`

### 8.5 Sequence number wrap-around

`packet_number` is `u32`. For 50 packets the counter never wraps, but the receive loop
must not assume monotonically increasing values because UDP can reorder packets:

```rust
// Use HashMap<u32, PacketRecord>, not a Vec indexed by seq
records.get_mut(&payload.packet_number)
```

### 8.6 Concurrency model

The TCP control channel (`UDPTEST`, `GET UDPRESULT`) stays on the MIO event loop thread.
The UDP send/receive loop runs in a dedicated blocking thread spawned via
`std::thread::spawn`. Results are passed back via `Arc<Mutex<Option<UdpQoSResult>>>`:

```rust
// Outgoing: send thread + receive loop on same thread (echo-based)
let send_thread = thread::spawn(move || {
    for i in 0..out_num_packets {
        let ts = now_ns();
        let payload = UdpPayload {
            communication_flag: FLAG_AWAIT_RESPONSE,
            packet_number: i as u32,
            uuid,
            timestamp_ns: ts as i64,
        };
        records.lock().unwrap().insert(i as u32, PacketRecord {
            packet_number: i as u32,
            send_time_ns: ts,
            deadline_ns: ts + tmax_ns,
            return_time: None,
        });
        socket.send_to(&payload.to_bytes(), server_addr).ok();
        thread::sleep(Duration::from_nanos(delay_ns));
    }
});

// Receive loop runs until all packets received or deadline passes
let deadline = Instant::now() + Duration::from_nanos(tmax_ns + out_num_packets * delay_ns);
loop {
    if Instant::now() >= deadline { break; }
    match socket.recv_from(&mut buf) {
        Ok((n, _)) => { /* classify echo */ }
        Err(e) if e.kind() == WouldBlock => continue,
        Err(_) => break,
    }
}
send_thread.join().ok();
```

---

## 9. Source Files

| File | Description |
|---|---|
| `src/udp/payload.rs` | `UdpPayload` (build/parse), `FLAG_*` constants, `now_ns()` |
| `src/udp/result.rs` | `PacketRecord`, `PacketOutcome`, `UdpQoSResult`, `calculate_qos()` |
| `src/udp/socket.rs` | `run_client_udp_out()`, `run_client_udp_in()`, `run_server_udp_out()`, `run_server_udp_in()` |
| `src/udp/mod.rs` | `UdpParams`, default constants |
| `src/mioserver/handlers/udp.rs` | Server MIO handlers: `UdpSendPort`, `UdpReceiveTestOut`, `UdpSendResultOut`, `UdpReceiveTestIn`, `UdpSendResultIn` |
| `src/client/handlers/udp.rs` | Client MIO handlers: 8 phases (GET UDPPORT → UDPRESULT IN) |
