# 🌐 Nettest - Network Speed Measurement

![Network Speed Measurement](hero.jpg)

## Overview

**Nettest** is a high-performance server and client for network speed measurement, written in Rust. The tool supports modern communication protocols and provides real-time accurate measurements with beautiful visualization.


![Nettest Demo](nettest-tcp.gif)


📚 **Complete Documentation**: [https://specure.github.io/nettest/docs](https://specure.github.io/nettest/docs)

## ✨ Key Features

### 🚀 **High Performance**
- **Multithreading** - Handle multiple clients simultaneously
- **Asynchronous architecture** - Efficient resource utilization
- **Connection queue** - Smart load distribution between workers

### 🌐 **Multi-Protocol Support**
- **TCP connections** - Direct connection for maximum performance
- **WebSocket** - Browser client support
- **TLS/SSL** - Secure connections

### 📊 **Real-Time Visualization**
- **Interactive speed graphs** - Real-time download and upload visualization
- **Beautiful UI** - Modern, responsive interface
- **Live measurements** - See your network performance in real-time

### 🔧 **Flexible Configuration**
- Configurable number of workers
- Configurable ports and addresses
- SSL/TLS certificate support

## 🗺️ Interactive Servers Map

<div style="text-align: center; margin: 30px 0; padding: 30px; background: linear-gradient(135deg, rgba(0, 212, 255, 0.1) 0%, rgba(0, 153, 204, 0.1) 100%); border: 2px solid rgba(0, 212, 255, 0.3); border-radius: 16px; position: relative; overflow: hidden;">
  <div style="position: absolute; top: 0; left: 0; right: 0; bottom: 0; background: url('https://specure.github.io/nettest/map-background.png') center/cover; opacity: 0.1;"></div>
  <div style="position: relative; z-index: 1;">
    <h2 style="margin: 0 0 15px 0; font-size: 28px; color: #00d4ff; text-shadow: 0 0 10px rgba(0, 212, 255, 0.3);">🌐 Interactive Measurement Interface</h2>
    <p style="margin: 0 0 25px 0; font-size: 18px; color: #e0e8ff; opacity: 0.9;">Experience real-time network measurements with beautiful visualization</p>
    <a href="https://specure.github.io/nettest" target="_blank" style="display: inline-block; padding: 15px 30px; background: linear-gradient(135deg, #00d4ff 0%, #0099cc 100%); color: white; text-decoration: none; border-radius: 12px; font-weight: 600; font-size: 16px; border: none; box-shadow: 0 4px 15px rgba(0, 212, 255, 0.3); transition: all 0.3s ease; text-shadow: 0 0 5px rgba(0, 212, 255, 0.3);">
      🚀 Launch Measurement Interface
    </a>
  </div>
</div>

## 🚀 Quick Start

### Download

Download the latest builds from the [GitHub Releases](https://github.com/specure/nettest/releases) page.

> **Note**:
> 1. Download the appropriate archive for your architecture and distribution
> 2. Extract:
>    - Linux/macOS: `tar -xzf nettest-<distribution>-<arch>.tar.gz`
>    - Windows: Extract the ZIP file
> 3. Run:
>    - Linux/macOS: `./nettest -s` (server) or `./nettest -c <address>` (client)
>    - Windows: `nettest.exe -s` (server) or `nettest.exe -c <address>` (client)

### Build

#### Local Build

```bash
# Debug build
cargo build

# Release build with optimizations
cargo build --release
```

#### GitHub Actions

The project includes automated builds via GitHub Actions:
- **Ubuntu builds**: Latest and LTS versions with native compilation
- **Debian builds**: Multiple versions (11, 12) for maximum compatibility
- **macOS builds**: Apple Silicon and Intel architectures
- **Windows builds**: x86_64 and ARM64 architectures

### Run Server

```bash
# Basic run
nettest -s
```

### Browser Speed Test (no setup)

`nettest -s` also serves a browser speed test on the measurement port. On start it prints the
addresses to open (and a QR code for phones):

```
  Nettest is running. Open the speed test on any device on this network:

    http://192.168.1.10:5005/
    http://nettest.local:5005/
```

Opening the page connects to the server it was loaded from and starts measuring right away
(ping, download, upload). No certificates, no configuration and nothing to install on the
device with the browser. The client is the same Rust measurement code as the CLI, compiled to
WebAssembly (the whole page is ~160 KB gzipped, embedded in the binary).

Notes:
- Plain HTTP is enough for ping, download and upload. Packet loss needs WebTransport, which
  browsers only allow on HTTPS (or `localhost`) pages; over plain HTTP jitter is derived from
  the ping round trips and the page leaves packet loss out.
- `?autostart=0` opens the page without starting a measurement. All client options are in the
  settings panel and can be passed as URL parameters ("Copy link with settings").
- Turn the page or the mDNS announcement off in `nettest.conf`:
  `enable_web_ui = false`, `enable_mdns = false`.

#### Where the speed goes (gateway or home server)

With `enable_upstream_test = true`, the page measures three legs, one after the other:

1. this device ↔ the nettest server (the normal browser test),
2. the server ↔ the internet: the server runs the nettest client itself against the nearest
   public measurement server (or `upstream_server`),
3. this device ↔ the internet: the browser measures the same public server directly.

It then says whether the Wi-Fi/LAN, the internet line or the server itself is the bottleneck,
with a This device / Server / Internet diagram, three concentric result rings on one scale and a
comparison table. The breakdown only runs for devices on the server's own network; visitors
from the internet get the plain test. Speeds don't add up: end to end should be close to the
slower leg; ping does add up. The
server also notes whether other devices used the line during its test and whether its CPU
was saturated. The `/api/upstream/*` endpoints only answer devices on the local network,
require an `X-Nettest: 1` header, reject cross-origin requests and allow one run per minute.

When the browser runs on the server itself, the local segment is only loopback and the
server's internet is this device's internet, so the page runs a single test: this device
against the nearest public measurement server.

`nettest -s -demo` (presentation mode) turns the upstream test on even without
`enable_upstream_test`, always runs all three segments whatever the path, and drops the
one-run-per-minute limit.

Rebuilding the browser client after changes to `web/` or `src/wasm/`:

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version <wasm-bindgen version from Cargo.lock>
./scripts/build-web.sh        # writes gzipped files to dist/ and dist.sha256 (wasm-opt optional)
cargo build --release         # embeds dist/
```

Commit `dist/` together with `dist.sha256`. CI runs `./scripts/build-web.sh --check` and fails
when `web/` or the WASM client changed without a rebuild.

### Run Client

```bash
# TCP client
nettest -c <SERVER_ADDRESS>

# WebSocket client
nettest -c <SERVER_ADDRESS> -ws

# TLS client 
nettest -c <SERVER_ADDRESS> -tls

# Machine-readable output
nettest -c <SERVER_ADDRESS> -json
```

### Output Formats

By default the client prints a table meant for humans. Two flags switch to
machine-readable output, which keeps stdout free of anything but the result:

| Flag | Output |
|------|--------|
| `-raw` | One line: `ping/download/upload`, latency in ms and speed in Gbit/s |
| `-json` | A JSON document with every measured value |

The JSON document reports nettest's native units: milliseconds for latency and
jitter, bits per second for speed, percent for packet loss and bytes for the
transferred volume. A value that was not measured is left out instead of being
reported as zero, so a consumer can tell "not measured" apart from "measured as
zero". With `-legacy`, for example, `jitter_ms` and `packet_loss_percent` are
absent because no VoIP and no UDP test ran.

```console
$ nettest -c 192.168.1.100 -json
{
  "type": "measurement",
  "timestamp": "2026-08-05T12:34:56Z",
  "client": {
    "name": "nettest",
    "version": "2.1.0"
  },
  "server": {
    "host": "192.168.1.100",
    "port": 5005
  },
  "protocol": "tcp",
  "num_threads": 3,
  "failed_threads": 0,
  "ping": {
    "latency_ms": 12.34,
    "jitter_ms": 0.42
  },
  "download": {
    "speed_bps": 942123456,
    "bytes_transferred": 1177654320
  },
  "upload": {
    "speed_bps": 512345678,
    "bytes_transferred": 640432098
  },
  "packet_loss_percent": 0.0
}
```

`jitter_ms` is the mean jitter measured by the VoIP test on an idle line. It is
not the jitter under download or upload load, because nettest does not measure
that.

Progress messages, warnings and errors go to stderr in every mode, so piping
stdout into a parser needs no filtering:

```bash
nettest -c 192.168.1.100 -json | jq .download.speed_bps
```

## ⚙️ Configuration

### Server Parameters

| Parameter | Description | Default |
|-----------|-------------|---------|
| `-l` | TCP listen address and port | `5005` |
| `-L` | TLS listen address and port | `443` |
| `-c` | Path to SSL certificate (PEM format) | - |
| `-k` | Path to SSL key file (PEM format) | - |
| `-udp` | UDP port for VoIP/packet loss tests | `5004` |
| `-u` | Drop privileges to specified user | - |
| `-d` | Run as daemon in background | `false` |
| `-t` | Number of worker threads | - |
| `-log` | Log level (info, debug, trace) | - |

### Client Parameters

| Parameter | Description | Default |
|-----------|-------------|---------|
| `-c` | Server address | auto-discover |
| `-tls` | Use TLS connection | `false` |
| `-ws` | Use WebSocket connection | `false` |
| `-t` | Number of threads | `3` |
| `-p` | TCP/TLS port | `5005` |
| `-g` | Generate graphs | `false` |
| `-raw` | Print one parseable line: `ping/download/upload` | `false` |
| `-json` | Print the measurement as JSON on stdout | `false` |
| `-legacy` | Use legacy PUT command (skip VoIP/packet loss) | `false` |
| `-log` | Log level (info, debug, trace) | - |

## 🔌 Protocols

### TCP Mode
Direct TCP connection for maximum performance:
```
Client <──TCP──> Server
```

### WebSocket Mode
Browser client support:
```
Client <──WebSocket──> Server
```

### TLS Mode
Secure connections:
```
Client <──TLS──> Server
```

## ⚡ Performance

Nettest is optimized for high performance:

- **Multithreading**: One server can support multiple clients
- **Asynchronous processing**: Efficient CPU and memory usage
- **Smart queue**: Automatic load distribution between workers
- **Minimal latency**: Optimized architecture for accurate measurements

## 📊 Visualization

### Real-Time Speed Graphs
- Live speed change visualization
- Detailed upload and download statistics
- Interactive charts with smooth animations

### Metrics
- **Download speed** - Real-time download performance
- **Upload speed** - Real-time upload performance
- **Ping** - Network round-trip time (median)
- **Jitter** - VoIP quality metric (RFC 3550), requires server v2.0+
- **Packet Loss** - UDP packet loss rate (RFC 6673), requires server v2.0+

> **Note**: Jitter and Packet Loss tests use UDP port `5004` by default.  
> Open this port on the server firewall: `iptables -A INPUT -p udp --dport 5004 -j ACCEPT`

## 📋 Requirements

### System Requirements
- **Rust**: 1.70+ (latest stable)
- **Linux/macOS/Windows**: Support for all major platforms

## 📄 License

- **Source code**: Apache License 2.0 ([LICENSE.txt](LICENSE.txt))


## 📚 Documentation

- [RMBT Protocol Specification](https://www.netztest.at/doc/)

---

**Nettest** - Your reliable tool for network speed measurement! 🚀
