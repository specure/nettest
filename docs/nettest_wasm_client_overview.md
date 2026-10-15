# Nettest in the Browser: the WASM Client
**How it is built, why, and the challenges solved along the way**  
*Branch: `feature/wasm-js-stream` — October 2026*

---

## What was built

* **The native Rust RMBT client now compiles to `wasm32-unknown-unknown`** and runs directly in the browser. JS calls a single function, `run_measurement(url, opts)`.
* **The browser runs the full measurement:** greeting $\rightarrow$ pre-test ping $\rightarrow$ download $\rightarrow$ upload $\rightarrow$ signed result $\rightarrow$ QoS (jitter and packet loss), and can optionally save the result to the control server.
* **The server gained a WebTransport QoS endpoint** on a separate UDP port (5006 by default).
* **The web wrapper moved to its own repository**, `specure/nettest-wasm-client`, which builds the WASM from this repository.

---

## Why

* **One engine for every client.** The browser runs the same RMBT handlers, the same speed calculator and the same RFC 3550 jitter as the CLI. The same Rust crate is also the engine of the Flutter library (`nettest-flutter-lib`), so web, mobile and desktop share one build. Their numbers are comparable by construction, not merely similar.
* **Shared fixes and tests.** A change in the shared code reaches every client at once, and the same `cargo test` suite covers the logic the browser runs.

---

## Why WebAssembly rather than JavaScript

* **No second implementation.** A JS client would be a rewrite of the RMBT protocol, the speed calculation and the QoS statistics, and would drift from the native one over time. WASM reuses the existing, tested Rust code as is.
* **Predictable execution speed.** WASM is compiled ahead of time and validated once: no JIT warm-up, no deoptimisation when a code path changes shape, no tiering mid-measurement. The first second of a test runs at the same speed as the last.
* **No garbage-collector pauses.** Memory is managed explicitly and buffers are reused, so the hot loop allocates nothing. In JS every received `ArrayBuffer` is garbage, and a GC pause in the middle of a transfer or a QoS train shows up as a dip in speed or a spike in jitter.
* **Exact integer arithmetic.** Timestamps are 64-bit nanoseconds. A JS `Number` is a double and is exact only up to $2^{53}$ nanoseconds since the epoch; exceed that, and JS would either lose precision or fall back to slow `BigInt`. Rust has native `u64`/`i64`, signed where transit times can go negative.
* **Close to the metal where it matters.** Byte counting, chunk-terminator detection and interpolation run as compiled code over linear memory, with no boxing or type checks. With the optimisations below, the full RMBT client matches a minimal JS client that only counts message lengths.
* **Type and memory safety.** The protocol state machine, the wire format and the units (bytes, nanoseconds, Mbit/s) are checked by the compiler rather than discovered at run time.
* **Small and portable.** After `wasm-opt -O3` the module is 127 KB. It runs in a page, in a Web Worker and in Node, since it uses only global timers and standard Web APIs.

---

## Challenges solved

### Performance
These optimisations made the browser client about 15% faster, confirmed by repeated runs in several browsers.

* **Backpressure on upload.** `WebSocket.send` never refuses data and has no drain event, so a naive upload loop would queue everything in the tab. The stream reports `WouldBlock` once `bufferedAmount` exceeds 512 KiB, the same signal a full native socket buffer gives, and the client hands control back to the browser.
* **Unclamped yields.** Chrome clamps nested `setTimeout` to about 4 ms, roughly 226 turns per second. The client yields through `MessageChannel` instead, about 390 000 turns per second, so the event loop, not a timer, sets the pace.
* **Zero-copy receive.** Incoming WebSocket messages are queued whole with a read cursor, and drained buffers are recycled. `Stream::consume` counts the bytes and checks the chunk terminator without copying the payload. The only copy left is the unavoidable one from JS memory into WASM, and the profile shows the client waiting on the network rather than on itself.
* **Parallel connections on one thread.** Three WebSockets run concurrently on the single JS event loop, and their samples are aggregated the same way as the native thread pool.

---

### QoS in the browser: jitter and packet loss

#### Why a separate transport
* **No UDP in the browser.** JavaScript has no API for a UDP socket, so the native VOIPTEST/UDPTEST phases cannot run there.
* **TCP hides loss.** Everything on the WebSocket is retransmitted: a lost packet shows up as a latency spike, never as a loss. A loss figure taken from a TCP stream would describe the transport, not the network.
* **WebTransport datagrams were chosen.** QUIC datagrams are never retransmitted, so real loss and jitter stay visible. WebRTC `DataChannel` would give the same data quality but needs a full $ICE + DTLS + SCTP$ stack and signalling.

| Transport | Real loss visible | Server cost |
| :--- | :--- | :--- |
| **WebTransport (HTTP/3) datagrams** | yes | a QUIC endpoint |
| **WebRTC DataChannel, maxRetransmits: 0** | yes | $ICE + DTLS + SCTP + \text{signalling}$ |
| **WebSocket (TCP)** | no | none |

#### How it works
* **Two channels.** The RMBT WebSocket arranges the test, and the packets travel over QUIC on UDP port 5006 (`/path/qos`). The commands mirror the native `UDPTEST`/`GET UDPRESULT`, without ports or NAT hole punching, because the QUIC session is already established and bidirectional:
  * `GET WTURL` $\rightarrow$ port, path, certificate hash, selfsigned trusted;
  * `WTTEST OUT` / `GET WTRESULT OUT` $\rightarrow$ received, duplicates, out-of-order, jitter;
  * `WTTEST IN` / `GET WTRESULT IN` $\rightarrow$ how many the server sent.
* **A 16-byte UUID binds the channels.** The QUIC session registers under it with its first datagram and waits for an echo; the control connection knows the session only by that identifier.
* **Same wire format and statistics as native.** The 29-byte payload of the native UDP test (flag, sequence number, UUID, send timestamp) and the same RFC 3550 estimator, so browser and native results are directly comparable.
* **Default train:** 10 packets per direction, 200 ms apart, plus a 1 s window for late packets, about 5 s for the phase. These match the native defaults.
* **Result.** The worse of the two directions, as in the native runner, plus per-direction counts, loss and jitter: loss in one direction is a different diagnosis from symmetric loss. Loss counts distinct sequence numbers, so a duplicated datagram cannot mask a lost one.

---

### Keeping browser noise out of the numbers

Jitter on a healthy link (1–5 ms) is the same order as browser scheduling noise, so each direction is measured by the side that can do it accurately.

| | OUT (client $\rightarrow$ server) | IN (server $\rightarrow$ client) |
| :--- | :--- | :--- |
| **Cadence set by** | client (irregular `setTimeout`) | server (precise `tokio` timer) |
| **Arrivals timestamped by** | server | client (`performance.now()`) |
| **Jitter computed by** | server, client send times subtracted | client |

* **The browser's own irregular sending is subtracted.** Each packet carries its send timestamp, and RFC 3550 subtracts the sender's spacing from the arrival spacing. With the client cadence deliberately randomised by $\pm 50\%$, the server still reported 0.12 ms of jitter: a hundred milliseconds of send-side wobble did not reach the result.
* **Unsynchronised clocks are harmless.** Only differences of transit times enter the formula, so a constant clock offset cancels out. Transit times are kept signed: when the server clock runs ahead, they are negative, and clamping them to zero would erase the jitter.
* **QoS runs alone and after the transfers.** The user sees throughput first. A 750 ms pause before the phase drains the queues that a just-saturated link still holds, which would otherwise inflate both jitter and loss.
* **Sub-millisecond timestamps.** `performance.now()` rather than `Date.now()`; under cross-origin isolation Chrome gives 5 µs resolution instead of 100 µs.
* **Sparse trains.** Ten packets 200 ms apart stay far below any congestion-control threshold, so the test measures the network and not QUIC's pacing.

---

### Engineering details

* **Server control handlers never block.** A `mio` worker serves many connections, so the handlers only read counters and push commands into a channel; a session registry by UUID bridges them to the `tokio` tasks that run QUIC.
* **Server jitter is accumulated per packet.** A train that loses packets never reaches its expected count; a "finalise on completion" design would report zero exactly when the network is worst.
* **The datagram reader cannot be abandoned.** A pending `reader.read()` cannot be raced against a timeout: the dropped promise stays in flight and the next read faults. All reads happen in one pump task that fills a buffer the phase inspects.
* **Certificates.** `serverCertificateHashes` accepts only short-lived P-256 certificates. The server generates one (14 days) when none is configured, always publishes the hash, and a flag tells the client whether to use it: applied to a normally trusted chain, the hash would fail a connection that plain verification accepts.
* **Hand-written WebTransport bindings**, to avoid the `web_sys_unstable_apis` flag for a small, stable slice of the spec.

---

## Fallback

* No WebTransport in the browser, the endpoint disabled, or UDP blocked by the network all end the same way: the reason is logged, and `qosTransport` in the result says which path produced the figures.
* **Jitter is then the same RFC 3550 estimator applied to the ping round trips** (up to 200 samples from the ping phase, at no extra cost). It is an approximation: RTT variation rather than one-way delay variation.
* **Packet loss is null**, not zero and not a guess.

---

## Validation (local server)

| Scenario | Expected | Measured |
| :--- | :--- | :--- |
| **Baseline, $10 \times 200\text{ ms}$** | 0% loss | 0%, jitter 0.1–0.3 ms |
| **Client sends 7 of 10 announced** | 30% OUT loss | 30.0% |
| **Client cadence randomised $\pm 50\%$** | jitter unaffected | 0.12 ms |
| **$50 \times 20\text{ ms}$ train (native VoIP shape)** | 0% loss | 0%, $50/50$ both ways |
| **Fallback, server RTT $15 \pm 2\text{ ms}$** | $\approx 4\text{ ms}$ | 4.00 ms |
| **Fallback, server RTT $20 \pm 10\text{ ms}$** | $\approx 20\text{ ms}$ | 20.00 ms |

*Not yet validated:* Injected loss and delay on the path itself (`tc netem`, `dnctl`/`pfctl`) and a side-by-side comparison with the native client under those conditions.

---

## Saving the result

* **Saving is opt-in** (`save: true` + `controlServer`) and goes to `/browser/save`, which needs no registration.
* **The client sends only what it knows itself.** Provider, ASN, geolocation and server identity are resolved server-side from the request address: a browser cannot be trusted to state them.
* **It sends the computed speeds and curve, not raw bytes.** Server-side aggregation would truncate every thread to the shortest one, and browser connections report at very different rates. Request body: 21 KB $\rightarrow$ 10.7 KB.