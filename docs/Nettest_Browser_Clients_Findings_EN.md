# Nettest in the Browser: Findings, Gotchas and How We Got Past Them

*Status: October 2026. All numbers are from loopback (`ws://localhost:5005`) on a
laptop (Intel Lunar Lake, 4P+4E cores), Firefox and Chromium in normal (headful)
mode on a virtual display, unless stated otherwise. Loopback numbers measure how
fast the **browser** can consume data, not a network; see Gotcha 1.*

## The goal

One `nettest` binary on the router:

1. measures the router's own link to the ISP (native client), and
2. serves a browser client over HTTP, so any device on the LAN can run a
   measurement **without installing anything**.

Two browser clients exist and speak the same, unchanged RMBT protocol over
WebSocket:

| Client | What it is | Where |
|---|---|---|
| **wasm client** | nettest's own Rust client code (`src/wasm/`, `src/client/`) compiled to WebAssembly | `nettest-wasm-client/wasm.html` + `pkg/` |
| **JS client** | A small hand-written JavaScript client | `nettest-wasm-client/index.html` + `rmbt-ws.js` |

## The story in one paragraph

The browser clients were at first 300× slower than the native client
(0.9 Gbit/s in Firefox against 292 Gbit/s native). Firefox **never got past the download phase**
and its tab grew to more than 6 GiB, until the OS killed it. None of it was down to the network or
to Rust vs JavaScript. The cause was **the way browsers receive WebSocket data**.
After fixing that entirely in the client, **with no change to the server protocol**,
both clients complete every phase in both browsers at 7–15 Gbit/s. Memory stays
bounded, and the 20 KB JS client now matches the 500 KB wasm client.

## Results: before and after

| | Before | After |
|---|---|---|
| Firefox, wasm client | download falls 3.9 → 2.2 Gbit/s, then times out; **no upload, no QoS**; tab grows ~500 MiB/s up to 6.3 GiB | full test: download ~7 Gbit/s, upload, QoS; total memory flat at ~2 GiB |
| Chromium, wasm client | download 10–12.8 Gbit/s, JS heap 0.9–2.9 GiB | download ~15 Gbit/s, JS heap ~150–170 MiB |
| Firefox, JS client | 0.9–3.2 Gbit/s, download only | ~7.8 Gbit/s, download + upload |
| Chromium, JS client | 3.6 Gbit/s, download only | ~12.9 Gbit/s, download + upload |

## Gotchas and how we overcame them

### Gotcha 1: "292 Gbit/s" isn't a network speed
On loopback there is no network card. The kernel copies memory from one socket
to the other, so the native number only shows how fast the machine can copy memory.
**Lesson:** loopback is a stress test for the *client*. Whether a browser keeps up has to be judged against
real link speeds (1, 2.5, 10 Gbit/s). At 7–15 Gbit/s the browser clients already
exceed any home connection.

### Gotcha 2: the bottleneck is per-message cost on one thread
Each WebSocket message goes through several steps before the page sees it:
1. It arrives in the browser's network process.
2. It is passed to the tab's process (IPC).
3. A buffer is allocated for it.
4. It is delivered as an event on the page's **single main thread**.

All connections share that thread, so **1, 3 or 6 connections give the same speed**.
The native client gets an OS thread per connection.

The same `.wasm` file runs at 26.7 Gbit/s in Node.js and ~14 Gbit/s in headless Chromium,
which uses the same V8 engine. So the limit is the browser's delivery path, not the engine or the code.

**What helped:** fewer, larger messages (bigger RMBT chunks); less work per
message; no DOM update per message. The JS client updated its progress bar on
every 4 KiB message, at ~250,000 updates/s. Throttling it to every 100 ms was part
of its 3–4× speedup.

### Gotcha 3: the server's default chunk size is a browser killer
The old JS client never asked for a chunk size, so it got the server default of
**4 KiB**. Each chunk is one WebSocket message, so that meant ~250,000 messages/s.
Requesting large chunks alone gave **~3× in Chromium (4.1 → 12.9 Gbit/s)**
and **~3.5× in Firefox (1.7 → 6.0 Gbit/s)**.

But bigger isn't always better. **Firefox handles 256 KiB messages better than 4 MiB**
(4.9 → 8.0 Gbit/s), while Chromium prefers 4 MiB. Both clients now cap chunks at 256 KiB in
Firefox only. This is a client-side choice; the protocol already allows it.

### Gotcha 4: Firefox buffers WebSocket data with no limit
With `GETTIME` the server sends as fast as it can for N seconds. Browsers handle that differently:
- **Chromium** stops reading from the socket when the page falls behind, so TCP slows the server down.
- **Firefox** keeps reading and queues every message inside the tab, with no limit.

On a fast link the queue grows by ~500 MiB/s. The server's final chunk sits at the end of
gigabytes of backlog, so the client times out and the test aborts. On a
machine with little free RAM, the OS's out-of-memory killer (`earlyoom`) ends the tab.

There's a second effect: the speed shown is how fast the tab *drains its queue*. That is
why the old JS client reported "2053 MB in 17.75 s" for a 7-second test.

**How we overcame it, without changing the server:** *bounded windows*. Instead of one
long `GETTIME`, the client repeatedly asks for a fixed amount of data with
`GETCHUNKS <n> <size>`, an existing command the server accepts at any prompt.
- At most ~32 MiB per connection (~96 MiB in total) is ever in flight.
- The window size adapts to the measured speed, so the gap between requests costs little.

Result:
- **Firefox:** total memory flat at ~2 GiB instead of up to 7.8 GiB, at the same speed.
- **Chromium:** download **+20–35%** and a JS heap ~6× smaller, because the browser no longer
  allocates and throws away gigabytes of buffers. It is now the default in all browsers.

Caveats:
- **Signed results** still use `GETTIME`. The server only signs bytes it sent through `GETTIME`.
- **Expected cost on high-latency links:** each window adds ~2 RTT of idle time. My estimate is
  up to ~10% at 1 Gbit/s and 50 ms RTT. **This still needs to be checked on an emulated link.**

### Gotcha 5: one failed phase threw away the whole test
If download failed on every connection, the wasm client aborted the measurement,
so users only ever saw "download". Now:
- A phase can fail on its own (`downloadError` / `uploadError`), and the test continues.
- Connections abandoned mid-download are closed (this also discards Firefox's backlog) and
  replaced with fresh ones for upload.
- The download ends on the client's own clock, not when the last byte arrives.

### Gotcha 6: browser timers lie at sub-millisecond scale
Firefox rounds `performance.now()` to 1 ms (anti-fingerprinting). The old JS client measured
ping in the page and took the minimum, so it showed `ping: 0.00 ms`. Both clients now use the RTT
the **server** measures and reports (`TIME <ns>`):
- taking the median,
- over up to 200 pings within 1 s, as in the native client.

The same idea applies to upload. With `PUTTIMERESULT` the **server** timestamps arriving data,
because the browser's own send buffer would otherwise inflate the result.

### Gotcha 7: two clients, two speed formulas
The JS client divided all bytes by the whole duration. Native and wasm use the
RMBT method:
- skip the first second (TCP slow start);
- end at *t\**, the earliest point at which every connection still had data;
- interpolate bytes at both ends.

The JS client now implements the same formula (`rmbtSpeedMbps`), so all three clients are comparable.

### Gotcha 8: the live display isn't the result
The Download card showed the live estimate (~1500 Mbit/s) until the
*whole* test ended. The wasm client now publishes each phase's final value as soon as that phase ends.

### Gotcha 9: measuring browsers is itself tricky
- **Run results vary a lot** on a loaded machine (Chromium: 6–14 Gbit/s on the same build).
  Compare medians of several runs.
- **Headless vs headful** turned out not to matter. Memory pressure did:
  with swap at 100%, the browser slows down and is eventually killed.
- **Every benchmark now runs in a memory-capped cgroup**
  (`systemd-run --scope -p MemoryMax=4G`), so a runaway tab can't take down
  the developer's desktop. This was learned the hard way.
- The repeatable benchmark lives in `nettest-wasm-client/bench/`. It drives the unmodified
  `wasm.html` in a real browser on Xvfb and reports speeds, the log and peak memory.

## wasm vs JS: size

Measured on the current builds (gzip -9 / brotli -11 are what a browser actually
downloads if the server sends compressed files):

| Asset | Raw | gzip | brotli |
|---|---|---|---|
| **JS client:** `rmbt-ws.js` + `index.html` | 24 KB | 8.8 KB | 7.5 KB |
| **wasm client:** `nettest_bg.wasm` (no `wasm-opt`) | 509 KB | 155 KB | 123 KB |
| `nettest_bg.wasm` after `wasm-opt -Oz` | 358 KB | 139 KB | 113 KB |
| + `pkg/nettest.js` glue + `wasm.html` | 39 KB | 10.3 KB | 8.7 KB |
| **wasm client total (optimised)** | ~397 KB | ~150 KB | ~122 KB |
| Today's embedded `dist/` (old JS UI) | 110 KB | 36 KB | – |
| `nettest` server binary (x86_64, release) | 13.4 MB | | |
| same, stripped | 9.9 MB | | |

**For the router:**
- The JS client adds **~0.1% to the stripped binary** (~9 KB pre-compressed).
- The optimised wasm client adds **~1.5% stored compressed (~150 KB)**, or ~4% if stored raw.
- Either is small next to the binary. Stripping symbols and setting `opt-level = "z"` / `lto` in
  a release profile would each save far more than the client costs.
  `Cargo.toml` has no `[profile.release]` tuning today.
- **Store the client pre-compressed** in the binary and serve it with `Content-Encoding: gzip`.
  This needs a small change to the static file handler. That handler also needs `application/wasm` as a MIME type
  and a real 404 for missing files.

## wasm vs JS: pros and cons

| | wasm client | JS client |
|---|---|---|
| **Code reuse** | Same Rust code as the native client: protocol, speed calculation, QoS, signed results. A fix lands everywhere | Separate implementation: every protocol detail and formula has to be kept in sync by hand |
| **Correctness drift** | Low: one implementation | Real risk: Gotchas 6 and 7 were exactly this |
| **Size** | ~120–150 KB compressed | ~8 KB compressed |
| **Speed (browser-bound)** | Firefox: 10–20% slower than JS, Chromium: equal | Slightly faster in Firefox |
| **Why** | Each message crosses JS → Rust → async task → JS; that per-message overhead is visible at ~30k msgs/s | Handles the message directly in `onmessage` |
| **Features today** | ping, download, upload, QoS (jitter/loss over WebTransport), signed results, adaptive windows | ping, download, upload |
| **Build** | Needs Rust wasm target, `wasm-bindgen` (pinned to the `Cargo.lock` version), ideally `wasm-opt` | None; edit and reload |
| **Debugging** | Harder: Rust panics, stack traces through glue code | Browser DevTools directly |
| **Browser support** | Any browser with WebAssembly (all current ones) | Any browser with WebSocket |
| **Where wasm would really pay off** | Heavy computation (crypto, compression, statistics) | – |

**Bottom line:** at these speeds the browser's WebSocket delivery path decides,
not the language. wasm's real advantage is **one codebase** shared with the
native client. JS's real advantage is **size and simplicity**. For the router, the size
difference (~150 KB) is affordable. What matters more is not maintaining two protocol
implementations.

## Remaining work

1. **Verify bounded windows on emulated links** (e.g. `tc netem`, 1/10 Gbit with
   10–100 ms RTT, in a network namespace) before keeping them as the default everywhere.
2. **Close the Firefox gap in the wasm client:** handle receive in JS and pass Rust
   only totals and timestamps.
3. **One Web Worker per connection:** the only way more connections can add speed
   inside one tab (Gotcha 2). Prototyped in the JS client; see
   [Beyond 10 Gbit/s](#beyond-10-gbits-parked) for results and the plan.
4. **Serve the client from the binary:**
   - embed `wasm.html` + `pkg/` (or the JS client), pre-compressed;
   - add `application/wasm`, a proper 404, and gzip encoding;
   - decouple the HTTP listener from the `-mdns` flag (today it only starts with `-mdns`, on TCP 5006).
5. **Add `wasm-opt -Oz`** to `build-wasm.sh` (−30% raw size).

## Beyond 10 Gbit/s (parked)

*Status: prototype measured on 2026-10-13, not implemented in either client.
Parked on purpose; pick up from "Plan" below.*

**Short answer:** yes in Chromium (one Web Worker per connection), no in Firefox
(the limit is inside the browser, not in the page).

### How it was measured
- Prototype in `nettest-wasm-client/bench/`:
  - `par-bench.html`: runs N connections either on the main thread or one per
    module Web Worker. Query: `?n=4&workers=1&dlms=5000&ulms=4000&chunk=262144&window=33554432`.
  - `par-worker.js`: the worker; reuses `connect`, `downloadWindowed` and
    `uploadPutTimeResult`, exported at the end of `rmbt-ws.js`.
  - Workers share the main page's time base via an absolute start time
    (`performance.timeOrigin + performance.now()`), so samples combine with
    `rmbtSpeedMbps` as usual.
- Headless Chromium and Firefox, loopback to `nettest -s` on the same laptop,
  bounded-window download, run in a 4 GiB memory cgroup. Single runs vary ±30%.

### Results (Mbit/s, three runs each)

| | Download | Upload |
|---|---|---|
| Chromium, main thread, 3 connections | 10353 / 9567 / 8788 | 26581 / 18727 / 18364 |
| **Chromium, 3 workers** | **15673 / 16035 / 14951** | 8154 / 9281 / 10600 ⚠ |
| Chromium, 4 workers | 15854 / 15847 / 10412 (one run: 23243) | 10732 / 11836 / 11268 ⚠ |
| Firefox, main thread, 3 connections | 5201 / 4730 / 5591 | 416 / 9821 / 7967 |
| Firefox, 3 workers | 6874 / 5559 / 6515 | 7295 / 4496 / 5904 |
| Firefox, 4 workers | 6700 / 6902 / 6422 (one run: 8815) | 7251 / 6828 / 6465 |

- Chromium with workers, by connection count: 1 → 9.0, 2 → 13.0, 4 → 15.6,
  6 → 12.5 Gbit/s. Beyond 4 the laptop runs out of cores (the server shares them).
- Firefox chunk size with workers: 256 KiB is still best (64 KiB ≈ equal at 6
  connections; 1 MiB and 4 MiB are ~2× slower).

### Why Chromium scales and Firefox doesn't (per-thread CPU, `top -H`)
- **Chromium:** 4 `DedicatedWorker` threads at ~55% each, the renderer's I/O
  thread at ~60%. Each worker has its own delivery path, nothing is saturated,
  so more workers add speed.
- **Firefox:** the **parent process main thread at 95–98%**, the `Socket Thread`
  at ~60%, the 4 `DOM Worker` threads only ~17% each. Every WebSocket message
  passes through that one parent thread, whichever page thread receives it.
- **Not WebSocket-specific:** a plain HTTP `fetch()` stream from a throwaway
  local server gave Firefox 7.1–7.6 Gbit/s on the main thread (and *less* in
  workers: 2.7–5.2), while Chromium went 10.8 → 22.6 Gbit/s with 4 workers.
  Switching transport would not lift the Firefox ceiling.
- Conclusion: Firefox tops out around **7–9 Gbit/s** on this laptop; only
  Mozilla can raise it. Workers still give Firefox **+25–30%** (≈5.2 → 6.7).

### The catch: upload is slower from workers
In Chromium, upload fell from 18–27 to 8–12 Gbit/s inside workers.
Unverified hypothesis: the send loop's 1 ms fallback sleep is clamped to
~4 ms for nested timers in workers, capping each connection at roughly one
512 KiB send buffer per sleep. Options:
- **Simple:** download in workers, upload on the main thread (already fast there).
- **To test:** a larger `SEND_HIGH_WATER` inside workers, or a `MessageChannel`
  yield in the worker instead of the timer.

### Plan (when picked up)
1. **JS client:** download in workers, upload on the main thread; test a larger
   send buffer for worker upload. Small; ~+50% download in Chromium.
2. **wasm client:** same split. Compile the module once and post it to each
   worker (`WebAssembly.Module` is transferable); each worker runs its
   connection's download; the main page combines samples with the existing
   RMBT calculation. Medium to large (`src/wasm/mod.rs` restructure).
3. **Firefox:** ship workers for the +25–30%, and report the parent-thread
   bottleneck to Mozilla with the `par-bench.html` reproducer.
4. Re-measure on a real 10G/25G link with the server on a separate machine,
   since loopback on one laptop understates the client-side headroom.
