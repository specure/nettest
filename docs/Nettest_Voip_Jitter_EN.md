# VoIP Jitter Measurement — nettest Implementation

This document describes how jitter is measured in the nettest client using a simulated
RTP stream (`src/voip/`).

See also: [Nettest_UDP_Packet_Loss_EN.md](Nettest_UDP_Packet_Loss_EN.md)

---

## Background: Standards for Jitter Measurement

Several IETF and ITU-T standards define how network jitter should be measured. Each
targets a different use case and level of rigour.

| Standard | Full name | Approach |
|---|---|---|
| **RFC 3550** | RTP: A Transport Protocol for Real-Time Applications | Exponential moving average over an RTP stream |
| **RFC 3393** | IP Packet Delay Variation (IPDV) | Statistical distribution of per-packet delay differences |
| **RFC 5357** | Two-Way Active Measurement Protocol (TWAMP) | Bidirectional RTT and delay variation via a dedicated reflector |
| **RFC 4656** | One-Way Active Measurement Protocol (OWAMP) | One-way delay metrics; requires synchronised clocks |
| **RFC 3611** | RTP Control Protocol Extended Reports (RTCP XR) | Extended RTCP statistics including jitter histograms |
| **ITU-T Y.1540 / Y.1541** | IP packet transfer and availability performance parameters | Normative thresholds built on IPDV (e.g. < 50 ms for VoIP) |

### RFC 3550 — exponential smoothing running estimate

Defined in Appendix A.8 of RFC 3550. Computes a single running jitter value using:

```
J(i) = J(i-1) + (|D(i-1,i)| - J(i-1)) / 16
```

where `D(i-1,i)` is the difference between the actual and expected inter-packet interval.
The result is a lightweight, monotonically-smoothed estimate suitable for real-time
feedback. It under-reports short bursts of jitter because the `/16` divisor dampens
spikes over several packets.

### RFC 3393 — IP Packet Delay Variation (IPDV)

The formal IETF definition of packet delay variation. Rather than a single running
estimate it produces a full statistical distribution: percentiles (p95, p99), min/max,
and histograms. This is more informative for network analysis but requires storing all
per-packet delay differences and a post-processing step. ITU-T Y.1540 uses IPDV as its
normative metric and sets the VoIP threshold at < 50 ms (p95).

### RFC 5357 — TWAMP

A complete active measurement protocol used by ISPs and telecoms. Requires a TWAMP
reflector on the far end, a binary control session, and is designed for operator-grade
infrastructure monitoring. Consumer devices cannot act as TWAMP reflectors, making this
unsuitable for an end-user test without a dedicated server role.

### RFC 4656 — OWAMP

Measures one-way delay and delay variation. Requires both endpoints to have synchronised
clocks (NTP or PTP with sub-millisecond accuracy). Consumer devices do not have
sufficiently precise clock synchronisation, so absolute one-way delay values would be
meaningless.

### RFC 3611 — RTCP XR

Extends RTCP with additional metrics including a jitter histogram. It is tightly coupled
to the WebRTC/SIP media stack. The jitter value exposed by browsers via
`RTCPeerConnection.getStats()` is computed using RTCP XR internally. However, it is a
black box from the application's perspective and cannot be produced from a raw UDP socket.

### Why RFC 3550 was chosen

RFC 3550 is the right fit for nettest for three reasons:

1. **Simulates the actual use case.** The test sends a CBR RTP stream at 20 ms intervals —
   the same cadence as a G.711 VoIP call. The resulting jitter value directly answers the
   question *"would a VoIP call be stable on this connection?"*, which is the regulatory
   goal (BEREC broadband quality guidelines).

2. **Works without clock synchronisation.** The `/16` recurrence formula operates on
   inter-packet intervals computed on the receiver's own monotonic clock. The clock offset
   between client and server cancels out algebraically, so no NTP or PTP is required.

3. **O(1) memory and single-pass computation.** The running estimate requires no buffering
   of all packets, making it suitable for embedded and mobile clients. RFC 3393 percentiles
   require O(n) storage and a sort; RFC 5357 and OWAMP require additional server
   infrastructure.

The trade-off is reduced sensitivity to short bursts: a single delayed packet has limited
impact on the smoothed estimate. For regulatory consumer-facing reporting this is
acceptable — the metric reflects perceived quality over the call duration rather than
worst-case network behaviour.

---

## 1. Parameters and Default Values

| Parameter | Config key | Default | Description |
|---|---|---|---|
| `call_duration` | `call_duration` | `1_000_000_000` ns (1 s) | Total stream duration |
| `delay` | `delay` | `20_000_000` ns (20 ms) | Inter-packet interval |
| `sample_rate` | `sample_rate` | `8000` Hz | Audio sample rate |
| `bits_per_sample` | `bits_per_sample` | `8` | Bits per sample |
| `payload_type` | `payload` | `PCMA` (8) | RTP codec (RFC 3551) |
| `buffer` | `buffer` | `100_000_000` ns (100 ms) | Jitter buffer size |
| `timeout` | `timeout` | `3_000_000_000` ns (3 s) | Receive timeout |
| `in_port` | `in_port` | 0 → dynamic | Client UDP receive port |
| `out_port` | `out_port` | `5004` | Server UDP port |

---

## 2. Protocol

The client sends a command over the existing TCP control connection:

```
VOIPTEST <out_port> <in_port> <sample_rate> <bits_per_sample> <delay_ms> <duration_ms> <initial_seq> <payload_type> <buffer_ns>
```

The server responds:
```
OK <ssrc>
```

Both sides then exchange RTP packets over UDP simultaneously:
- **outgoing** (client → server): client sends RTP packets to the server's `out_port`
- **incoming** (server → client): server sends RTP packets to the client's `in_port`

After the stream completes, the client requests the server-side results:
```
GET VOIPRESULT <ssrc>
```

The server responds:
```
VOIPRESULT <max_jitter> <mean_jitter> <max_delta> <skew> <num_packets> <seq_errors> <short_seq> <long_seq> <stalls> <avg_stall_time>
```

All jitter and timing values are in nanoseconds.

---

## 3. RTP Packet Generation

Number of packets in the stream:
```
num_packets = duration_ms / delay_ms
```

Payload size per packet:
```
payload_size = sample_rate * delay_ms / 1000 * bits_per_sample / 8
```

Example with default values:
```
num_packets  = 1000 / 20 = 50 packets
payload_size = 8000 * 20 / 1000 * 8 / 8 = 160 bytes
```

RTP timestamp increment per packet (`src/voip/mod.rs`):
```rust
pub fn num_packets(&self) -> u64 {
    self.duration_ms / self.delay_ms
}

pub fn payload_size(&self) -> usize {
    (self.sample_rate as u64 * self.delay_ms / 1000
        * self.bits_per_sample as u64 / 8) as usize
}

pub fn timestamp_increment(&self) -> u32 {
    (self.sample_rate as u64 * self.delay_ms / 1000) as u32
}
```

Each packet carries a pseudo-random payload — the byte content is irrelevant to the measurements.

---

## 4. RTP Header Structure (12 bytes, big-endian)

```
 0                   1                   2                   3
 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|V=2|P|X|  CC   |M|     PT      |       Sequence Number         |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                           Timestamp                           |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|           Synchronization Source (SSRC) identifier           |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

- **V** (2 bits, bits 7–6 of byte 0): version = 2
- **P** (1 bit, bit 5 of byte 0): padding = false
- **X** (1 bit, bit 4 of byte 0): extension = false
- **CC** (4 bits, bits 3–0 of byte 0): CSRC count = 0
- **M** (1 bit, bit 7 of byte 1): marker = true for the first packet only
- **PT** (7 bits, bits 6–0 of byte 1): payload type (e.g. PCMA = 8)
- **Sequence Number** (bytes 2–3, uint16 big-endian): random initial value (0–9999), +1 per packet
- **Timestamp** (bytes 4–7, uint32 big-endian): starts at 0, incremented by `timestamp_increment`
- **SSRC** (bytes 8–11, uint32 big-endian): issued by the server in `OK <ssrc>`

Serialisation and parsing in `src/voip/rtp.rs`:
```rust
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
    if data.len() < RTP_HEADER_SIZE { return None; }
    Some(Self {
        marker:          (data[1] & 0x80) != 0,
        payload_type:    data[1] & 0x7f,
        sequence_number: u16::from_be_bytes([data[2], data[3]]),
        timestamp:       u32::from_be_bytes([data[4], data[5], data[6], data[7]]),
        ssrc:            u32::from_be_bytes([data[8], data[9], data[10], data[11]]),
        payload:         data[RTP_HEADER_SIZE..].to_vec(),
    })
}
```

---

## 5. Incoming Packet Collection

For each received RTP packet only the fields needed for QoS calculation are stored
(`src/voip/rtp.rs`):

```rust
pub struct RtpControlData {
    pub sequence_number: u16,
    pub rtp_timestamp:   u32,
    pub received_ns:     u64,  // monotonic ns: thread_local Instant::elapsed()
}

pub type PacketMap = HashMap<u16, RtpControlData>;
```

The map key is `sequence_number`. Duplicate sequence numbers are silently discarded —
only the first arrival is kept.

---

## 6. Jitter Calculation Algorithm (RFC 3550, Appendix A.8)

Implemented in `src/voip/calculator.rs` as `calculate_qos()`.

### Step 1. Sort by sequence number

```rust
let mut by_seq: Vec<_> = packets.values().collect();
by_seq.sort_by_key(|p| p.sequence_number);
```

### Step 2. Compute delta for each consecutive pair

For packets `prev` and `cur`, delta is the absolute difference between the actual and
expected inter-packet interval:

```
real_diff_ns     = received_ns[cur] - received_ns[prev]
expected_diff_ns = trunc_ms( (ts[cur] - ts[prev]) / sample_rate * 1000 ) * 1_000_000
delta            = |real_diff_ns - expected_diff_ns|
```

```rust
let real_diff_ns = cur.received_ns as i64 - prev.received_ns as i64;

// Truncate to whole milliseconds before converting to ns — matches RTP timer resolution.
let ts_diff_ms = (cur.rtp_timestamp.wrapping_sub(prev.rtp_timestamp) as f32
    / sample_rate as f32
    * 1000.0) as i64;
let expected_diff_ns = ts_diff_ms * 1_000_000i64;

let delta = (real_diff_ns - expected_diff_ns).unsigned_abs() as i64;
```

`wrapping_sub` is required because the RTP timestamp is an unsigned 32-bit counter.

### Step 3. Exponential smoothing

The running jitter estimate starts at zero for the first packet, then for each subsequent one:

```rust
jitter += (delta as f32 - jitter) / 16.0;
```

This is the RFC 3550 recurrence formula `J(i) = J(i-1) + (|D(i-1,i)| - J(i-1)) / 16`.

The intermediate value is `f32`. Truncation to `i64` happens via `as i64` — not rounding.

### Step 4. Aggregate final metrics

```rust
max_jitter   = max_jitter.max(jitter as i64);  // truncation, not rounding
mean_jitter += jitter as i64;                  // sum over n-1 pairs
max_delta    = max_delta.max(delta);

// Divide by n (total received packets), not n-1 (pairs).
// The first packet contributes 0.0 to the sum but is included in the count.
let n = by_seq.len() as i64;
if n > 0 { mean_jitter /= n; }
```

All three values are in **nanoseconds**.

### Step 5. Skew (clock drift)

```rust
skew += expected_diff_ns - real_diff_ns;
```

A positive `skew` means the RTP stream is arriving faster than the sender's clock suggests.
Measured in nanoseconds.

### Step 6. Stall detection

A stall is recorded when the actual gap between two packets exceeds the jitter buffer size:

```rust
if real_diff_ns > buffer_ns as i64 {
    stalls += 1;
    stall_time += real_diff_ns - buffer_ns as i64;
}

let avg_stall_time = if stalls > 0 {
    stall_time / 1_000_000 / stalls as i64  // ns → ms, then average
} else {
    0
};
```

`avg_stall_time` is in **milliseconds**.

### Step 7. Out-of-order analysis

A second pass over the same packets, sorted by arrival time, checks whether sequence
numbers appear in the expected order:

```rust
let mut by_time: Vec<_> = packets.values().collect();
by_time.sort_by_key(|p| p.received_ns);

let mut next_expected:  u16 = initial_seq;
let mut out_of_order:   i32 = 0;
let mut cur_sequential: i32 = 0;
let mut max_sequential: i32 = 0;
let mut min_sequential: i32 = 0;  // 0 = not yet observed

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
```

`update_min` keeps the smallest observed sequential run, ignoring the initial zero:

```rust
fn update_min(cur: i32, min: i32) -> i32 {
    if cur < min { cur } else if min == 0 { cur } else { min }
}
```

Sequential run counts are capped at the total number of received packets:

```rust
let received = packets.len() as i32;
min_sequential: if min_sequential > received { received } else { min_sequential },
max_sequential: if max_sequential > received { received } else { max_sequential },
```

### Result structure

```rust
// src/voip/calculator.rs
pub struct RtpQoSResult {
    pub received_packets:  usize,
    pub max_jitter:        i64,  // ns
    pub mean_jitter:       i64,  // ns
    pub skew:              i64,  // ns
    pub max_delta:         i64,  // ns
    pub out_of_order:      i32,
    pub min_sequential:    i32,
    pub max_sequential:    i32,
    pub number_of_stalls:  i32,
    pub avg_stall_time:    i64,  // ms
}
```

Result fields are reported with directional prefixes:
- Incoming stream (client receives from server): `voip_result_in_<metric>`
- Outgoing stream (server received from client): `voip_result_out_<metric>`

---

## 7. Units of Measurement

| Metric | Internal unit | Output unit |
|---|---|---|
| `max_jitter` | ns (i64) | ns |
| `mean_jitter` | ns (i64) | ns |
| `max_delta` | ns (i64) | ns |
| `skew` | ns (i64) | ns |
| `avg_stall_time` | ms (i64) | ms |
| `delay` | ns internally, ms in server command | — |
| `timeout` | ns internally | — |

For display, convert from nanoseconds to milliseconds:
```rust
let jitter_ms = max_jitter as f64 / 1_000_000.0;
```

---

## 8. Implementation Notes

### 8.1 `f32` for jitter, `i64` for output

The running jitter estimate uses `f32` throughout. Truncation to `i64` happens at two points:

```rust
max_jitter  = max_jitter.max(jitter as i64);
mean_jitter += jitter as i64;
```

Using `f64` changes the numerical result slightly and should be avoided.

### 8.2 Two separate sort passes

The `PacketMap` is traversed twice with different orderings. This is intentional:

```rust
// Pass 1 — jitter: sorted by sequence number (measures timing variation)
let mut by_seq: Vec<_> = packets.values().collect();
by_seq.sort_by_key(|p| p.sequence_number);

// Pass 2 — out-of-order: sorted by arrival time (measures reordering)
let mut by_time: Vec<_> = packets.values().collect();
by_time.sort_by_key(|p| p.received_ns);
```

### 8.3 RTP timestamp wrap-around

The RTP timestamp is `u32`. Always use `wrapping_sub`:

```rust
let ts_diff = cur.rtp_timestamp.wrapping_sub(prev.rtp_timestamp);
```

### 8.4 Sequence number wrap-around

Use `u16` and `wrapping_add` when advancing the expected sequence counter:

```rust
next_expected = next_expected.wrapping_add(1);
```

### 8.5 Millisecond-precision timestamp conversion

Truncation to whole milliseconds before converting to nanoseconds is intentional —
it matches RTP timer resolution:

```rust
let ts_diff_ms = (ts_diff as f32 / sample_rate as f32 * 1000.0) as i64;
let expected_diff_ns = ts_diff_ms * 1_000_000i64;
```

### 8.6 Concurrency model

The TCP control channel (VOIPTEST, GET VOIPRESULT) stays on the MIO event loop thread.
The UDP exchange runs in a dedicated blocking thread spawned via `std::thread::spawn`.
Results are passed back via `Arc<Mutex<Option<RtpQoSResult>>>`:

```rust
// Sender thread
let send_thread = thread::spawn(move || {
    for i in 0..num_packets {
        let pkt = RtpPacket::new(seq, ts, ssrc, payload_type, i == 0, payload_size);
        send_socket.send_to(&pkt.to_bytes(), remote_addr).ok();
        seq = seq.wrapping_add(1);
        ts  = ts.wrapping_add(ts_increment);
        thread::sleep(delay);
    }
});

// Receiver loop — current thread, blocks until deadline
let deadline = Instant::now() + Duration::from_millis(duration_ms + 3000);
loop {
    if Instant::now() >= deadline { break; }
    match recv_socket.recv_from(&mut buf) { ... }
}

send_thread.join().ok();
```

---

## 9. Source Files

| File | Description |
|---|---|
| `src/voip/rtp.rs` | `RtpPacket` (build/parse), `RtpControlData`, `PacketMap`, `now_ns()` |
| `src/voip/calculator.rs` | `calculate_qos()` — RFC 3550 algorithm; `RtpQoSResult` |
| `src/voip/udp.rs` | `run_server_udp()`, `run_client_udp()` — blocking UDP exchange threads |
| `src/voip/mod.rs` | `VoipParams`, default constants |
| `src/mioserver/handlers/voip.rs` | Server MIO handlers: `VoipSendOk`, `VoipSendResult` |
| `src/client/handlers/voip.rs` | Client MIO handlers: 4 phases (send command → receive result) |
