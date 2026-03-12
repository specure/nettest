# Performance Comparison of the RMBT Protocol: Legacy C-Version vs. Modern Rust Implementation

## Introduction: What is RMBT?

RMBT (Reference Monitoring and Broadband Test) is an open-source, multi-threaded protocol developed for high-precision measurement of network quality parameters (QoS/QoE). Unlike standard HTTP-based tests, RMBT operates directly at the TCP socket level using specific data chunk exchange logic and measurement phase control. This architecture minimizes browser-induced overhead and ensures compliance with strict regulatory standards, such as those defined by BEREC.

For a long time, the industry standard was the reference server implementation in C ([rtr-nettest/rmbt-server](https://github.com/rtr-nettest/rmbt-server)). However, in modern production environments, maintaining this stack has become increasingly difficult:

- **Memory management and multi-threading in C** always carry inherent risks. Implementing new features in legacy code often feels like a "walk through a minefield," where a single mistake can lead to memory leaks or critical security vulnerabilities.
- **We sought a tool that could provide "out-of-the-box" Memory Safety** without compromising performance. Our hypothesis was that a modern Rust model (based on MIO) would not only improve the codebase but also demonstrate superior throughput. This model allows receiving read/write events directly from the kernel and interacting with sockets without additional libraries. The entire server is built as a state machine that maintains state between kernel/epoll events.

---

## The Technical Debt of the Legacy C Implementation

Beyond the inherent risks of manual memory management, the legacy C implementation (`rmbtd.c`) faces significant challenges due to dependency rot and the use of outdated programming patterns:

- **OpenSSL Legacy API:** The C server is tightly coupled with older versions of OpenSSL (often 1.1.x). With OpenSSL 1.1.1 having reached End-of-Life (EOL), migrating to OpenSSL 3.0+ in a manual C codebase is a massive undertaking. The new APIs are often incompatible with the way legacy code handles SSL structures, leading to security risks or the continued use of unpatched, vulnerable libraries.

- **Manual POSIX Threading:** The implementation relies on raw `pthreads` and low-level POSIX socket headers. While functional, this approach lacks the efficiency of modern asynchronous runtimes. Scaling to 100+ Gbps requires sophisticated CPU affinity handling and non-blocking I/O, which in C must be written and maintained entirely by hand, unlike the mature ecosystem of Rust's `tokio` or `mio`.

- **Lack of Modern Event Loops:** Analysis of `rmbtd.c` shows a heavy reliance on manual `epoll` management and custom buffer handling. In the modern era, these "reinvented wheels" are difficult to audit for security. Modern frameworks have moved toward structured concurrency, which is nearly impossible to retrofit into the existing C architecture.

- **Maintenance Stagnation:** The repository has entered a "maintenance-only" phase. Finding developers who can safely modify high-performance network C code without introducing regressions is becoming increasingly difficult, making the stack a liability for long-term production use.

---

## Research Objective

This article presents a comparative analysis of the original C server and the new implementation in Rust ([specure/nettest](https://github.com/specure/nettest)). We aim to identify the strengths and weaknesses of each implementation using identical client software and a high-end hardware testbed.

---

## Testing Methodology

To ensure an objective comparison, we prepared an isolated environment to eliminate network fluctuations.

### Hardware Specifications

| Component | Details |
|-----------|---------|
| Processor | AMD Ryzen AI MAX+ 395 (16 cores, 32 threads) |
| RAM | 32 GB LPDDR5 (28 GB available for testing) |
| Operating System | Ubuntu 24.04.3 LTS |
| Kernel | Linux 6.14.0-generic |
| Architecture | x86-64 |

---

## Technical Configuration

### C-Server Modifications

To run the C server locally without a control server, the following source code modifications were implemented:

1. **Disable token validation** in `rmbtd.c` (line 817): Replace `"if (r != 3)"` with `"if (CHECK_TOKEN && r != 3)"`.
2. **Remove response rate limits** in `rmbtd.c` (line 1051): Delete `"|| (diffnsec - last_diffnsec > 1e6)"`. This ensures the server responds immediately to every chunk.
3. **Disable tokens** in `config.h`: Set `"#define CHECK_TOKEN 0"`.

The C server is started with the following command:

```bash
./rmbtd -L 443 -l 8080 -c specure-cd.crt -k specure-cd.key -w
```

### Rust Server & Client Setup

Start the Rust server:

```bash
./nettest -s
```

Adjust the client chunk size in `/etc/nettest.conf` to match the iperf standard:

```
max_chunk_size = 131072
```

---

## Benchmark Results

### 1. Plain TCP Throughput

| Command | Description |
|---------|-------------|
| `./nettest -c 127.0.0.1 -t 20 -legacy -p 8080` | C-server test |
| `./nettest -c 127.0.0.1 -t 20` | Rust-server test |
| `iperf3 -c 127.0.0.1 -P 20` | iperf3 baseline |

| Implementation | Download (Gbit/s) | Upload (Gbit/s) |
|----------------|:-----------------:|:---------------:|
| RMBT (C) | 718.19 | 943.23 |
| RMBT (Rust) | **1047.12** | **1013.24** |
| iperf3 (Baseline) | 993.00 | 993.00 |

### 2. TLS Throughput

| Command | Description |
|---------|-------------|
| `./nettest -c 127.0.0.1 -t 20 -tls` | Rust TLS |
| `./nettest -c 127.0.0.1 -t 20 -legacy -p 443 -tls` | C TLS |

| Implementation (TLS) | Download (Gbit/s) | Upload (Gbit/s) |
|----------------------|:-----------------:|:---------------:|
| RMBT (Rust) | **331.22** | **327.46** |
| RMBT (C) | 279.83 | 276.05 |

### 3. WebSocket (WS) Throughput

| Command | Description |
|---------|-------------|
| `./nettest -c 127.0.0.1 -t 20 -ws` | Rust WS |
| `./nettest -c 127.0.0.1 -t 20 -legacy -p 8080 -ws` | C WS |

| Implementation (WS) | Download (Gbit/s) | Upload (Gbit/s) |
|---------------------|:-----------------:|:---------------:|
| RMBT (Rust) | **382.68** | **387.79** |
| RMBT (C) | 334.99 | 88.90 |

### 4. WebSocket + TLS (WS+TLS) Throughput

| Command | Description |
|---------|-------------|
| `./nettest -c 127.0.0.1 -t 20 -ws` | Rust WS+TLS |
| `./nettest -c 127.0.0.1 -t 20 -legacy -p 8080 -ws` | C WS+TLS |

| Implementation (WS+TLS) | Download (Gbit/s) | Upload (Gbit/s) |
|-------------------------|:-----------------:|:---------------:|
| RMBT (Rust) | **272.11** | **252.71** |
| RMBT (C) | 241.75 | 75.33 |

---

## Data Analysis

Our tests reveal that the C server delivered the lowest performance metrics. In the download phase, which utilizes identical protocol logic and calculation methods, the C server was **more than 30% slower**.

The upload phase highlights a key architectural difference. The Rust implementation uses a slightly modified protocol where the server transmits all intermediate results at the end of the measurement phase rather than after every single chunk. This reduces network overhead significantly. iperf3 and the Rust RMBT implementation showed comparable results, with RMBT maintaining a slight performance edge.

---

## Conclusion

The transition to Rust is fully justified. The new implementation excels in high-throughput local measurements and offers superior scalability. Beyond performance, the move to a modern codebase simplifies maintenance and future-proofs the protocol for upcoming broadband standards.

---

## Technical Observations and Future Work

### 1. TCP Upload Chunk Size Insensitivity (C Server)

During development and benchmarking, we observed that the TCP upload phase on the C server is almost entirely insensitive to changes in the chunk size. Performance remained consistently static regardless of the adjustments made in the client configuration. This suggests a potential bottleneck within the legacy implementation — possibly a hardcoded internal buffer size or a fixed window in the legacy socket handling logic. This phenomenon requires a more granular code investigation to determine if it's an inherent limitation of the C server's architecture or an artifact of our legacy integration.

### 2. WebSocket Upload Performance and Protocol Overhead

The WebSocket (WS) upload phase for the C server stands out due to its significantly lower performance compared to all other test types (dropping to ~89 Gbit/s). Our primary hypothesis points to the legacy protocol structure: the C server transmits measurement results immediately after every individual chunk, which effectively "clogs" the stream with control messages at high speeds. There may also be room for optimization in how the server/client handles the WebSocket framing itself. In contrast, the new protocol implementation (RMBT Protocol Extensions) mitigates this by consolidating results, leading to the vastly superior results seen in the Rust implementation.
