# Supplement: RMBT Java Client Performance Analysis — C Server vs. Rust Server Baseline

> This document is a supplement to [RMBT_Full_Technical_Comparison_EN.md](./RMBT_Full_Technical_Comparison_EN.md).  
> All tests were conducted on the same hardware testbed described in the primary document.

---

## Motivation

The primary comparison document establishes a clear performance advantage of the modern Rust server over the legacy C server. A natural follow-up question arises: **how does the Java reference client fit into this picture?**

The Java RMBT client ([rtr-nettest/open-rmbt — RMBTClient](https://github.com/rtr-nettest/open-rmbt/tree/master/RMBTClient)) is the canonical desktop/CLI implementation historically used to validate RMBT server behavior. By pairing it with the legacy C server — the original reference combination — we can establish a third data point that illuminates how much of the performance gap is attributable to the server and how much to the client.

We also ran the Java client against the Rust server, but the results showed no meaningful difference compared to the Java + C server combination: the bottleneck is entirely on the client side. Because the server had no measurable impact on Java client throughput.

---

## Why the Java Client Is Expected to Underperform the Rust Client

Even before measuring, several structural properties of the JVM ecosystem predict a performance ceiling below what a native Rust client can achieve at extreme throughputs:

### 1. Garbage Collection Pauses
The Java client allocates `byte[]` buffers for each chunk and relies on the GC to reclaim them. At multi-hundred-Gbit/s throughput the allocation rate is enormous. GC pauses — even short ones from a modern G1 or ZGC collector — directly manifest as measurement gaps: the receive/send loop stalls while the collector runs, and the peer can only use its window. Rust uses deterministic, scope-based memory management with no GC whatsoever.

### 2. No Zero-Copy I/O
Java's `InputStream`/`OutputStream` model over plain `Socket` introduces mandatory copies between kernel socket buffers and JVM heap arrays. Rust's `mio`-based client interacts directly with kernel events and can leverage `sendfile(2)` / scatter-gather I/O semantics, bypassing extra copy steps on the hot path.

### 3. SSL/TLS Implementation Depth
Java's `SSLSocket` wraps `SSLEngine`, which adds an extra stateful layer on top of the OS TLS stack. Rust's `rustls` (or native-tls) integrates more tightly with the I/O event loop, avoiding redundant buffer copies during the TLS handshake and record framing steps.

---

## Reproducing the Java Client Test: Engineering Challenges

### The `--token` Bypass Bug (NPE)

The Java CLI client supports a `--token` flag intended to skip the control-server round-trip and run standalone. In the version under test (`RMBTClient` from the `open-rmbt` repository), **this path is broken**: passing `--token` results in a `NullPointerException` at runtime.


### Workaround: Hardcoded Control-Server Response

To proceed without a live control server, we patched the client to return a static JSON string in place of the HTTP request:

```java
// Intended bypass — does NOT work; throws NPE on subsequent JSONObject access
// final JSONObject response = JSONParser.sendJSONToUrl(hostUrl, regData);
```

```java
final String jsonString = "{\n" +
    "    \"test_uuid\": \"c5378ee8-7084-417a-b18b-b51c44abbcf4\",\n" +
    "    \"result_url\": \"https://api.nettest.org/measurementResult\",\n" +
    "    \"result_qos_url\": \"https://api.nettest.org/measurementQosResult\",\n" +
    "    \"test_duration\": 7,\n" +
    "    \"test_server_name\": \"NKOM1\",\n" +
    "    \"test_wait\": 0,\n" +
    "    \"test_server_address\": \"127.0.0.1\",\n" +
    "    \"test_numthreads\": 20,\n" +
    "    \"test_server_port\": 443,\n" +
    "    \"test_server_encryption\": true,\n" +
    "    \"test_token\": \"c5378ee8-7084-417a-b18b-b51c44abbcf4_1743749015_3d3RaJ0B8wj5H1XG/cKSCl6B3AE=\",\n" +
    "    \"test_numpings\": 10,\n" +
    "    \"test_id\": 4497960,\n" +
    "    \"client_remote_ip\": \"31.146.70.177\",\n" +
    "    \"provider\": \"JSC Silknet\",\n" +
    "    \"app_version\": \"3.1.2\",\n" +
    "    \"platform\": \"UNKNOWN\",\n" +
    "    \"error\": []\n" +
    "}";
// Replaces: final JSONObject response = JSONParser.sendJSONToUrl(hostUrl, regData);
```

The parameters that were varied between test runs:

| Parameter | TCP test | TLS test |
|-----------|:--------:|:--------:|
| `test_numthreads` | 20 | 20 |
| `test_server_port` | 8080 | 443 |
| `test_server_encryption` | `false` | `true` |

The C server was started identically to the primary document:

```bash
./rmbtd -L 443 -l 8080 -c specure-cd.crt -k specure-cd.key -w
```

---

## Benchmark Results: Java Client + C Server

All tests run on the same machine described in the primary document (AMD Ryzen AI MAX+ 395, Ubuntu 24.04.3 LTS, Linux 6.14.0-generic).

### Plain TCP Throughput (Java Client → C Server, port 8080)

```
Total calculated bytes down: 116,018,532,322
Total calculated time down:  7.000 s
Total calculated bytes up:   440,187,901,370
Total calculated time up:    7.007 s

Total Down: 132,592,275 kBit/s  →  132.59 Gbit/s
Total Up:   502,587,139 kBit/s  →  502.59 Gbit/s
Ping:       0.01 ms
```

### TLS Throughput (Java Client → C Server, port 443)

```
Total calculated bytes down: 110,564,012,032
Total calculated time down:  7.000 s
Total calculated bytes up:   135,479,512,922
Total calculated time up:    7.000 s

Total Down: 126,358,638 kBit/s  →  126.36 Gbit/s
Total Up:   154,824,074 kBit/s  →  154.82 Gbit/s
Ping:       0.01 ms
```

---

## Combined Comparison

### Plain TCP

| Client → Server | Download (Gbit/s) | Upload (Gbit/s) |
|-----------------|:-----------------:|:---------------:|
| Java → C | 132.59 | 502.59 |
| Rust → C | 718.19 | 943.23 |
| Rust → Rust | **1047.12** | **1013.24** |

### TLS

| Client → Server | Download (Gbit/s) | Upload (Gbit/s) |
|-----------------|:-----------------:|:---------------:|
| Java → C | 126.36 | 154.82 |
| Rust → C | 279.83 | 276.05 |
| Rust → Rust | **331.22** | **327.46** |

---

## Analysis

### TCP Download: The Most Striking Gap

The Java client achieved only **132.59 Gbit/s** on TCP download — roughly **5.4× slower** than the Rust client talking to the same C server (718.19 Gbit/s). This is a client-side bottleneck: the server is identical, yet throughput collapses. The most likely cause is the combination of JIT warm-up latency and GC pressure on the receive path. Because the Java client must copy each arriving chunk from the kernel socket buffer into a JVM heap array, and then the GC must track and eventually collect millions of short-lived arrays, the effective receive window shrinks and the sender is throttled.

### TCP Upload: A Relative Bright Spot

Upload (502.59 Gbit/s) fared considerably better relative to the download, reaching **53% of the Rust client's upload rate** against the same server (943.23 Gbit/s). On the send path the Java client can pre-allocate a reusable buffer and fill it in a tight loop. There is no inbound data to copy, so GC pressure is lower and the hot path stays JIT-compiled throughout the run.

### TLS: Symmetric Degradation

Under TLS both directions deteriorate to a similar level (~126–155 Gbit/s), erasing the upload advantage seen in plain TCP. The `SSLEngine` record framing and the extra heap copies it introduces appear to equalise the allocation rate between send and receive, making GC the universal bottleneck regardless of direction. The Rust client, with its tighter TLS integration and zero-copy buffers, sustains **2.2× the download** and **2.1× the upload** over the same C server.

### Implication for Server Evaluation

A key takeaway is that **the choice of client can misrepresent server capability by a factor of 5×**. Had the original RMBT benchmarking ecosystem only ever measured using the Java client, the true server-side headroom would have remained invisible. The Rust client is necessary to saturate modern high-core-count hardware and reveal real server limits.

---

## Conclusion

The Java RMBT client, while historically important as the reference implementation, is a significant performance bottleneck in extreme-throughput environments. At loopback speeds on modern multi-core hardware, JVM overhead — GC pauses, mandatory heap copies, JIT warm-up, and `SSLEngine` framing — limits effective throughput to a fraction of what a native Rust client achieves against the identical server.

The data reinforces the conclusion from the primary document: replacing both the C server and the legacy Java client with the modern Rust stack yields **the highest throughput across all tested transport modes**, and is the only combination capable of approaching and exceeding the iperf3 baseline on the same hardware.

The broken `--token` standalone mode in the Java client also highlights a broader maintenance concern: the Java codebase, like the C server, has entered a state where basic developer-workflow paths are silently broken, further undermining confidence in it as a long-term testing tool.
