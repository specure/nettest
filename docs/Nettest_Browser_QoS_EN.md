# Jitter and Packet Loss in the Browser (WASM) Client — nettest Implementation

How the browser client measures jitter and packet loss, why it needs a transport of its own, and
what the numbers mean.

This document is the browser counterpart of `Nettest_Voip_Jitter_EN.md` (RTP/UDP jitter) and
`Nettest_UDP_Packet_Loss_EN.md` (UDP packet loss). The statistics are deliberately the same code, so
browser results are comparable with native ones rather than merely similar; what differs is the
transport and the constraints a browser imposes on timing.

---

## Background: why the native path does not apply

The native client measures QoS over UDP: an RTP train for jitter (`VOIPTEST`) and a counted
datagram train for loss (`UDPTEST OUT/IN`). A browser cannot do either:

* **No UDP socket.** JavaScript has no API for one, and none is planned.
* **TCP hides loss.** Everything on the control channel — the WebSocket — is retransmitted by TCP.
  A packet lost on the wire is re-sent transparently and shows up as a latency spike, never as a
  loss. Any "packet loss" figure derived from a TCP stream describes the transport, not the network.

Three transports can carry unreliable traffic from a browser:

| Transport | Real loss visible | Server cost | Browser coverage |
| --- | --- | --- | --- |
| **WebTransport (HTTP/3), datagrams** | yes — QUIC datagrams are never retransmitted | a QUIC endpoint (`wtransport`) | Chrome, Edge, Firefox; Safari recent |
| WebRTC DataChannel, `maxRetransmits: 0` | yes | full ICE + DTLS + SCTP stack and signalling | all, including older Safari |
| WebSocket (TCP) | **no** | none | all |

nettest uses **WebTransport datagrams**, with the WebSocket path kept as a degraded fallback (see
§8). WebRTC would add an ICE/DTLS/SCTP stack and its own pacing for the same data quality.

---

## 1. Parameters and Default Values

| Parameter | Default | Where | Notes |
| --- | --- | --- | --- |
| QUIC endpoint port (UDP) | `5006` | `server_wt_port` in `nettest.conf` | separate from RMBT TCP (5005) and native UDP QoS (5004) |
| Endpoint enabled | `true` | `enable_webtransport` | when off, the control channel answers "no endpoint" |
| URL path | `/qos` | `wtqos::WT_PATH` | other paths are refused with 404 |
| Packets per direction | `10` | client option `qosPackets` | same as `DEFAULT_UDP_OUT_NUM_PACKETS` |
| Inter-packet delay | `200 ms` | client option `qosDelayMs` | same as `DEFAULT_UDP_DELAY_NS` |
| Late-packet window (Tmax) | `1000 ms` | `qos::TMAX_MS` | same as `DEFAULT_UDP_TMAX_NS` |
| Settle time before the phase | `750 ms` | `wasm::QOS_SETTLE_MS` | see §7.4 |
| Registration timeout | `3000 ms` | `qos::REGISTER_TIMEOUT_MS` | |
| Control command timeout | `5000 ms` | `qos::COMMAND_TIMEOUT_MS` | |
| QUIC idle timeout / keep-alive | `30 s` / `3 s` | `wtqos::endpoint` | a QoS phase lasts seconds |

A run with the defaults takes about 2 s per direction plus Tmax, so roughly 5 s in total.

---

## 2. Protocol

The RMBT control channel (WebSocket) arranges the test; the packets themselves travel over QUIC.
The commands mirror the native `UDPTEST` / `GET UDPRESULT` pair, without ports or NAT hole punching
— the QUIC session is already established and bidirectional.

```
client                                                     server
  │  GET WTURL                                                  │
  │ ─────────────────────────────────────────────────────────►  │
  │  WTURL <port> <path> <cert-sha256-base64> <selfsigned|trusted>
  │ ◄─────────────────────────────────────────────────────────  │
  │                                                             │
  │  ═══ open WebTransport session to https://host:port/path ═══│
  │  datagram: flag=0 (register), uuid                          │
  │ ─────────────────────────────────────────────────────────►  │
  │  datagram: flag=2 (echo)                                    │
  │ ◄─────────────────────────────────────────────────────────  │
  │                                                             │
  │  WTTEST OUT <n> <delay_ms> <uuid_hex>          →  OK        │
  │  n datagrams, flag=1, seq 0..n-1, own send timestamps       │
  │ ─────────────────────────────────────────────────────────►  │
  │  (wait Tmax)                                                │
  │  GET WTRESULT OUT                                           │
  │  RCV <received> <duplicates> <out_of_order> <jitter_ns> <max_delta_ns>
  │ ◄─────────────────────────────────────────────────────────  │
  │                                                             │
  │  WTTEST IN <n> <delay_ms> <uuid_hex>           →  OK        │
  │  n datagrams, flag=1, server send timestamps                │
  │ ◄─────────────────────────────────────────────────────────  │
  │  (wait n·delay + Tmax)                                      │
  │  GET WTRESULT IN                               →  SNT <sent>│
```

The 16-byte UUID is what ties the two channels together: the control connection knows the session
only by that identifier, and the QUIC session registers itself under it with its first datagram.
A `WTURL 0 - - -` reply means the endpoint is disabled or failed to start.

### 2.1 Certificates

The endpoint always publishes the SHA-256 of its leaf certificate — it is public handshake data —
together with a word saying whether the client *needs* it:

* **`selfsigned`** — the client passes the hash in `serverCertificateHashes`. Browsers accept that
  only for short-lived ECDSA P-256 certificates, which is exactly what the server generates when no
  certificate is configured (14 days, P-256). This is what makes local development work.
* **`trusted`** — the client connects normally; the certificate must be valid for the host the page
  connects to, exactly like the WSS one.

Using the hash against a normally trusted chain would *fail* a connection that plain verification
accepts, which is why the hint travels with the hash.

---

## 3. Packet Structure

The wire format is the native UDP QoS payload (`src/udp/payload.rs`), unchanged — 29 bytes,
big-endian:

| Offset | Size | Field | Meaning |
| --- | --- | --- | --- |
| 0 | 1 | `communication_flag` | 0 register, 1 measurement packet, 2 echo/response |
| 1 | 4 | `packet_number` | sequence number within the train, from 0 |
| 5 | 16 | `uuid` | session identifier shared with the control channel |
| 21 | 8 | `timestamp_ns` | sender's clock when the packet was handed to the socket |

Reusing the format is not just tidiness: `timestamp_ns` is what makes the OUT direction measurable
from a browser at all (§6.1).

---

## 4. Direction Handling

The two directions are measured by different sides, and deliberately so:

| | OUT (client → server) | IN (server → client) |
| --- | --- | --- |
| Cadence controlled by | client (`setTimeout`, irregular) | server (`tokio::time::sleep`, precise) |
| Arrival timestamps taken by | server | client (`performance.now()`) |
| Loss computed by | client, from `RCV received` against what it sent | client, from distinct sequence numbers against `SNT sent` |
| Jitter computed by | server, transit times with client send times subtracted | client, transit times |

---

## 5. Packet Loss Algorithm

Both directions use the same simple ratio; the count of *distinct* sequence numbers is what
counts, so a duplicated datagram cannot mask a lost one:

```
loss_percent = 100 · (sent − received) / sent
```

* **OUT** — `sent` is what the client actually put on the wire, `received` is the server's count of
  distinct sequence numbers (`RCV` field 1). Duplicates and out-of-order arrivals are reported
  separately (`RCV` fields 2 and 3) rather than folded into loss.
* **IN** — `sent` is the server's own count of datagrams handed to QUIC (`SNT`), `received` is the
  number of distinct sequence numbers the client saw within `n · delay + Tmax`.

The reported figure is the **worse of the two directions**, matching the native runner:

```rust
pub fn packet_loss_percent(&self) -> f64 {
    self.out.loss_percent().max(self.inbound.loss_percent())
}
```

Per-direction numbers are also returned (§9). Loss in one direction only is a different diagnosis
from symmetric loss, and the combined figure hides it.

---

## 6. Jitter Algorithm

RFC 3550 §6.4.1 interarrival jitter, the same estimator and smoothing constant the native VoIP test
uses, so the two are comparable:

```
D(i−1, i) = (R_i − R_{i−1}) − (S_i − S_{i−1})
J_i       = J_{i−1} + (|D(i−1, i)| − J_{i−1}) / 16
```

where `S` is a send time (from the payload) and `R` the corresponding arrival time.

Because only *differences* of transit times enter the formula, the constant offset between two
unsynchronised clocks cancels: the machines do not need NTP agreement, only stable clocks.

The shared implementation is `client::calculator::rfc3550_jitter_ns` (used by the client and by the
WebSocket fallback); the server accumulates the same estimator packet by packet in
`wtqos::endpoint::Jitter`.

### 6.1 Why the OUT direction is measurable at all

A browser cannot send at a fixed cadence: `setTimeout` is clamped (≈4 ms once timers nest, and up to
1 s in a background tab), and any long task delays the next send. Measuring arrival deltas alone
would report the browser's scheduler, not the network — tens of milliseconds of pure artefact.

The `D` formula above subtracts the sender's own irregularity, and the sender's timestamps ride
along in every packet. Validation of exactly this, from the JS probe (`wtqos-test.html`), which can
randomise its own cadence on demand:

| Client send cadence | Jitter reported by the server |
| --- | --- |
| steady 200 ms | 0.99 ms |
| randomised ±50 % (100–300 ms) | **0.12 ms** |

A hundred milliseconds of deliberate send-side wobble does not reach the result.

### 6.2 Server-computed vs client-computed

The server accumulates OUT jitter incrementally rather than in a final pass. A train that loses
packets never reaches its expected count, and the control channel must still find a usable figure
when the client asks — a "finalise on completion" design would report zero exactly when the network
is worst.

---

## 7. Accuracy Constraints in a Browser

Jitter on a healthy link (1–5 ms) is the same order as browser scheduling noise, so the phase is
built to keep that noise out of the numbers.

### 7.1 Timer resolution
`performance.timeOrigin + performance.now()` is used for timestamps rather than `Date.now()`:
sub-millisecond resolution is what keeps a millisecond-scale figure meaningful. Chrome rounds
`performance.now()` to 100 µs by default, and to 5 µs under cross-origin isolation (`COOP:
same-origin` + `COEP: require-corp`) — worth setting for a page whose job is measurement.

### 7.2 Event loop
The QoS phase runs alone: never next to a transfer, which would measure our own scheduling. Reads
of the datagram stream happen in a dedicated pump task, so nothing else can stall them.

### 7.3 Background tabs
Timer throttling in a hidden tab is severe (1 s). A page that may be backgrounded should check
`document.visibilityState` and mark such a result as unreliable.

### 7.4 Placement after the transfers
The client runs QoS **after** download and upload, so a user sees throughput first. That costs some
accuracy the native order (before the transfers) avoids: a link that has just been saturated still
has full queues, which inflates both jitter and loss. A `QOS_SETTLE_MS` pause (750 ms) drains them
before the phase starts; on links with deep buffers this may need raising.

### 7.5 QUIC pacing
Ten packets 200 ms apart are far below any congestion-control threshold. Dense trains (hundreds of
packets per second) would start measuring QUIC's pacing rather than the network.

---

## 8. Fallback When WebTransport Is Unavailable

No WebTransport in the browser, the endpoint disabled, or UDP blocked by the network — all end the
same way: the client logs the reason and reports

* **jitter** as the RFC 3550 estimator applied to the RMBT ping round trips (up to 200 samples
  collected during the ping phase, at no extra cost). It is an approximation: RTT variation rather
  than one-way delay variation, and blind to retransmissions.
* **packet loss** as `null` — not zero, not a guess. Over TCP the browser cannot see loss at all,
  and a retransmission-derived number would describe the transport.

`qosTransport` in the result says which of the two produced the figures.

---

## 9. Result Structure

```json
{
  "jitterMs": 1.8,
  "packetLossPercent": 0,
  "qosTransport": "webtransport",
  "qos": {
    "out": { "sent": 10, "received": 10, "lossPercent": 0, "jitterMs": 1.6 },
    "in":  { "sent": 10, "received": 10, "lossPercent": 0, "jitterMs": 1.8 }
  }
}
```

| Field | Meaning |
| --- | --- |
| `jitterMs` | worse of the two directions |
| `packetLossPercent` | worse of the two directions; `null` when not measurable |
| `qosTransport` | `"webtransport"`, `"websocket"` (fallback), or `null` |
| `qos.out` / `qos.in` | per-direction counts, loss and jitter |

---

## 10. Units of Measurement

| Quantity | On the wire | In the result |
| --- | --- | --- |
| Timestamps | nanoseconds, `i64`, big-endian | — |
| Jitter | nanoseconds (`RCV` field 4) | milliseconds, `f64` |
| Max delta | nanoseconds (`RCV` field 5) | not surfaced |
| Loss | counts (`RCV`, `SNT`) | percent, `f64` |

---

## 11. Validation

Measured against a local server; the loopback floor for this setup is ≈0.1–0.2 ms of jitter, which
is the noise level any real figure must be read against.

| Scenario | Expected | Measured |
| --- | --- | --- |
| Baseline, 10 × 200 ms | 0 % loss both directions | 0 %, jitter 0.1–0.3 ms |
| Client sends 7 of 10 announced | 30 % OUT loss | **30.0 %** |
| Client cadence randomised ±50 % | jitter unaffected | 0.12 ms (vs 0.99 ms steady) |
| 50 × 20 ms train (native VoIP shape) | 0 % loss | 0 %, 50/50 both directions |
| Endpoint disabled | fallback, loss `null` | fallback taken, reason logged |
| Fallback path, server reporting constant RTT | 0 ms | **0.00 ms** |
| Fallback path, server reporting 15 ± 2 ms | ≈4 ms | **4.00 ms** |
| Fallback path, server reporting 20 ± 10 ms | ≈20 ms | **20.00 ms** |

Still outstanding: validation with injected loss and delay on the path itself (`tc netem` on Linux,
`dnctl`/`pfctl` on macOS) and a side-by-side comparison with the native client under those
conditions.

---

## 12. Implementation Notes

### 12.1 File map

Server (this repository):

| File | Role |
| --- | --- |
| `src/wtqos/endpoint.rs` | QUIC endpoint, one task per session, both directions via `select!` |
| `src/wtqos/registry.rs` | sessions by UUID; the bridge between tokio tasks and mio workers |
| `src/mioserver/handlers/wtqos.rs` | `GET WTURL`, `WTTEST`, `GET WTRESULT` |
| `src/udp/payload.rs`, `src/client/calculator.rs` | wire format and statistics, shared with the native client |

Client — the Rust half is compiled from this repository, the pages live in
[nettest-wasm-client](https://github.com/specure/nettest-wasm-client):

| File | Repository | Role |
| --- | --- | --- |
| `src/wasm/qos.rs` | this one | the phase itself |
| `src/wasm/mod.rs` | this one | driver, phase order, result assembly |
| `src/stream/wt_datagrams.rs` | this one | WebTransport bindings (hand-written, to avoid `web_sys_unstable_apis`) |
| `wasm.html` | nettest-wasm-client | client UI |
| `wtqos-test.html` | nettest-wasm-client | plain-JS probe for the transport (drop / cadence knobs) |

### 12.2 Control handlers never block

A mio worker thread serves many connections. The QoS control handlers only read counters and push
commands into a channel that the QUIC session task drains — no waiting for a train to finish.

### 12.3 The datagram reader cannot be abandoned

`reader.read()` on a WebTransport datagram stream cannot be raced against a timeout: dropping the
pending promise leaves it in flight, and the next `read()` faults. All reads therefore happen in one
pump task that appends arrivals to a buffer the phase inspects.

### 12.4 Chunk of the native test not carried over

RTT statistics (`rtt_avg/min/max`, per-packet RTTs) that the native UDP test derives from its
`AWAIT_RESPONSE` flag are not implemented on the WebTransport path — the browser phase measures the
two one-way trains only. Burst-loss episodes (`max_burst_loss`, `loss_episodes`) are likewise not
computed yet, though the shared code in `src/udp/result.rs` is ready for them.
