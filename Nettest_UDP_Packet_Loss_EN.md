# UDP Packet Loss Measurement — nettest Implementation

This document describes how UDP packet loss and RTT are measured in the nettest client.

See also: [Nettest_Voip_Jitter_EN.md](Nettest_Voip_Jitter_EN.md)

---

## 1. Parameters

| Parameter | Config key | Default | Description |
|---|---|---|---|
| `out_num_packets` | `out_num_packets` | — | Packet count client→server |
| `in_num_packets` | `in_num_packets` | — | Packet count server→client |
| `out_port` | `out_port` | via `GET UDPPORT` | Server port for outgoing stream |
| `in_port` | `in_port` | dynamic | Client port for incoming stream |
| `delay` | `delay` | `300_000_000` ns (300 ms) | Inter-packet interval |
| `timeout` | `timeout` | `3_000_000_000` ns (3 s) | Receive timeout |

---

## 2. Protocol

### Outgoing test (client → server)

1. `GET UDPPORT` → server responds with `<port>`
2. `UDPTEST OUT <port> <packet_count>` → server responds `OK`
3. Client sends `packet_count` UDP packets to the server
4. `GET UDPRESULT OUT <port>` → server responds `RCV <received_count> <port>`

### Incoming test (server → client)

1. Client binds a local UDP socket on `in_port`
2. `UDPTEST IN <in_port> <packet_count>` → server starts sending packets
3. `GET UDPRESULT IN <in_port>` → server responds `RCV <received_count> <port> [<json_rtts>]`

---

## 3. Packet Structure

Each UDP datagram carries a `UdpPayload`:

| Field | Type | Description |
|---|---|---|
| `communication_flag` | u8 | `1` = ONE_DIRECTION, `2` = RESPONSE, `3` = AWAIT_RESPONSE |
| `packet_number` | u16/u32 | Sequence number, 0-based |
| `uuid` | bytes | Client identifier |
| `timestamp` | i64 | Monotonic nanoseconds at send time |

The client sends with `AWAIT_RESPONSE (3)`; the server echoes back the same payload with
`RESPONSE (2)`, enabling RTT calculation on the client side.

---

## 4. Packet Tracking

The client maintains three collections per direction:

```rust
packets_received:  HashSet<u32>       // unique packet numbers
duplicate_packets: HashSet<u32>       // duplicate arrivals (tracked separately)
rtt_map:           HashMap<u32, u64>  // packet_number → RTT in ns
```

On each received packet:

```rust
let packet_number = parse_packet_number(payload);
let rtt = now_ns() - payload.timestamp as u64;

if packets_received.contains(&packet_number) {
    duplicate_packets.insert(packet_number);
    // duplicate — not counted toward num_packets, test continues
} else {
    packets_received.insert(packet_number);
    rtt_map.insert(packet_number, rtt);
}
```

Duplicates do not affect `num_packets` or the packet loss rate — only unique arrivals are
counted.

---

## 5. Packet Loss Rate

### Outgoing (client → server)

The server reports how many packets it received. The client computes:

```rust
let lost = sent_count.saturating_sub(server_received);
let packet_loss_rate = if lost == 0 {
    0i32
} else {
    (lost as f32 / sent_count as f32 * 100.0) as i32
};
```

If the server received more packets than were sent (e.g. due to duplicates), loss is
clamped to zero via `saturating_sub`.

Result key: `udp_result_out_packet_loss_rate` — integer 0–100 (percent).

### Incoming (server → client)

```rust
let lost = expected_count.saturating_sub(server_sent);
let packet_loss_rate = (lost as f32 / expected_count as f32 * 100.0) as i32;
```

The divisor is `expected_count` (the originally requested packet count), **not**
`server_sent`. If the server sent fewer packets than requested, the shortfall is counted
as loss against the expected total.

Result key: `udp_result_in_packet_loss_rate` — integer 0–100 (percent).

---

## 6. RTT Metrics

RTT is available for the outgoing direction only (the server echoes back each packet with
`RESPONSE (2)`, so the round-trip time is measured at the client):

```rust
let rtt_avg_ns = rtt_map.values().sum::<u64>() / rtt_map.len() as u64;
```

| Result key | Description |
|---|---|
| `udp_result_out_rtt_avg_ns` | Average RTT, outgoing stream, in ns |
| `udp_result_in_rtt_avg_ns` | Average RTT, incoming stream, in ns |
| `udp_result_out_rtts_ns` | Per-packet RTTs: `BTreeMap<u32, u64>` |
| `udp_result_in_rtts_ns` | Per-packet RTTs: `BTreeMap<u32, u64>` |

---

## 7. Units of Measurement

| Metric | Internal unit | Output unit |
|---|---|---|
| `packet_loss_rate` | % (i32, 0–100) | % |
| RTT | ns (u64) | ns |
| `delay` | ns internally, ms in server command | — |
| `timeout` | ns internally | — |

---

## 8. Data Structures

```rust
pub struct UdpPacketData {
    pub num_packets:         usize,
    pub dup_num_packets:     usize,
    pub rcv_server_response: usize,
    pub rtts:                BTreeMap<u32, u64>,  // packet_number → rtt_ns
}

pub fn packet_loss_rate(sent: usize, received: usize) -> i32 {
    if received >= sent { return 0; }
    ((sent - received) as f32 / sent as f32 * 100.0) as i32
}
```
