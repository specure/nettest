![Network Speed Measurement](Gemini_Generated_Image_skkcnfskkcnfskkc.png)

## Overview

Nettest is a high-performance server and client for network speed measurement, written in Rust. The tool supports modern communication protocols and provides real-time accurate measurements.

## Key Features

### 🌐 **Multi-Protocol Support**
- **TCP connections** - Direct connection for maximum performance
- **WebSocket** - Browser client support
- **TLS/SSL** - Secure connections

### ⚡ **High Performance**
- **Multithreading** - Handle multiple clients simultaneously
- **Asynchronous architecture** - Efficient resource utilization
- **Connection queue** - Smart load distribution between workers

### 📊 **Data Visualization**
- **Time speed change graphs**
- **Detailed measurement statistics**

### 🔧 **Flexible Configuration**
- Configurable number of workers
- Configurable ports and addresses
- SSL/TLS certificate support

## Quick Start

### Download

Download the latest builds directly:

#### Ubuntu
- **Ubuntu 24.04 x86_64**: [nettest-ubuntu-24-x86_64.tar.gz](https://github.com/specure/nettest/releases/download/latest/nettest-ubuntu-24-x86_64.tar.gz)
- **Ubuntu 24.04 ARM64**: [nettest-ubuntu-24-aarch64.tar.gz](https://github.com/specure/nettest/releases/download/latest/nettest-ubuntu-24-aarch64.tar.gz)
- **Ubuntu 22.04 x86_64**: [nettest-ubuntu-22-x86_64.tar.gz](https://github.com/specure/nettest/releases/download/latest/nettest-ubuntu-22-x86_64.tar.gz)
- **Ubuntu 22.04 ARM64**: [nettest-ubuntu-22-aarch64.tar.gz](https://github.com/specure/nettest/releases/download/latest/nettest-ubuntu-22-aarch64.tar.gz)

#### Debian
- **Debian 12 (Bookworm) x86_64**: [nettest-debian-12-x86_64.tar.gz](https://github.com/specure/nettest/releases/download/latest-debian-12/nettest-debian-12-x86_64.tar.gz)
- **Debian 12 (Bookworm) ARM64**: [nettest-debian-12-aarch64.tar.gz](https://github.com/specure/nettest/releases/download/latest-debian-12/nettest-debian-12-aarch64.tar.gz)
- **Debian 11 (Bullseye) x86_64**: [nettest-debian-11-x86_64.tar.gz](https://github.com/specure/nettest/releases/download/latest-debian-11/nettest-debian-11-x86_64.tar.gz)
- **Debian 11 (Bullseye) ARM64**: [nettest-debian-11-aarch64.tar.gz](https://github.com/specure/nettest/releases/download/latest-debian-11/nettest-debian-11-aarch64.tar.gz)

#### macOS
- **macOS Apple Silicon**: [nettest-macos-aarch64.tar.gz](https://github.com/specure/nettest/releases/download/latest-macos/nettest-macos-aarch64.tar.gz)
- **macOS Intel**: [nettest-macos-x86_64.tar.gz](https://github.com/specure/nettest/releases/download/latest-macos/nettest-macos-x86_64.tar.gz)

> **Note**: 
> 1. Download the appropriate archive for your architecture and distribution
> 2. Extract: `tar -xzf nettest-<distribution>-<arch>.tar.gz`
> 3. Run: `./nettest -s` (server) or `./nettest -c <address>` (client)

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

### Run Server

```bash
# Basic run
nettest -s

```

### Run Client

```bash
# TCP client
nettest -c <SERVER_ADDRESS>

# WebSocket client
nettest -c <SERVER_ADDRESS> -ws

# TLS client 
nettest -c <SERVER_ADDRESS> -tls
```

## Configuration

### Server Parameters

| Parameter | Description | Default |
|-----------|-------------|---------|
| `-l` | TCP listen address and port | `5005` |
| `-L` | TLS listen address and port | `443` |
| `-c` | Path to SSL certificate (PEM format) | - |
| `-k` | Path to SSL key file (PEM format) | - |
| `-u` | Drop privileges to specified user | - |
| `-d` | Run as daemon in background | `false` |
| `-log` | Log level (info, debug, trace) | - |

### Client Parameters

| Parameter | Description | Default |
|-----------|-------------|---------|
| `-c` | Server address | `127.0.0.1` |
| `-tls` | Use TLS connection | `false` |
| `-ws` | Use WebSocket connection | `false` |
| `-t` | Number of threads | `3` |
| `-p` | Port number | `8080` |
| `-g` | Generate graphs | `false` |
| `-log` | Log level (info, debug, trace) | - |

## Protocols

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

## Performance

Nettest is optimized for high performance:

- **Multithreading**: One server can support multiple clients
- **Asynchronous processing**: Efficient CPU and memory usage
- **Smart queue**: Automatic load distribution between workers
- **Minimal latency**: Optimized architecture for accurate measurements

## Visualization

### Speed Graphs
- Speed change visualization
- Detailed upload and download statistics

### Metrics
- Download speed
- Upload speed
- Latency

### 🗺️ **Active Servers Map**
Interactive map showing all active measurement servers with their locations:

- **Real-time data**: Shows all registered servers with valid coordinates
- **Interactive markers**: Click on markers to see server details
- **Auto-refresh**: Updates every 30 seconds
- **Server information**: Name, city, distance, version, and capabilities

<details>
<summary>📊 **View Active Servers Map**</summary>

<div style="height: 500px; width: 100%; border: 1px solid #ddd; border-radius: 5px; overflow: hidden;">
  <iframe 
    src="https://specure.github.io/nettest/servers_map.html" 
    style="width: 100%; height: 100%; border: none;"
    title="Measurement Servers Map">
  </iframe>
</div>

### 🚀 **Setup Options**

#### **Option 1: GitHub Pages (Recommended)**
The map is automatically deployed to GitHub Pages and available at:
**https://specure.github.io/nettest/servers_map.html**

No local server setup required - just open the URL in your browser!

#### **Option 2: Local Development**
To view the map locally with your control server:

1. **Start your control server** (make sure it's running on the configured port)
2. **Run the local server**: `./serve_map.sh`
3. **Open your browser** and go to: `http://localhost:8000/servers_map.html`

> **Note**: The map connects to the control server API to fetch real-time server data. Make sure your control server is accessible at the configured URL.
</details>

## Requirements

### System Requirements
- **Rust**: 1.70+ (latest stable)
- **Linux/macOS/Windows(?)**: Support for all major platforms


## License

- **Source code**: Apache License 2.0 ([LICENSE.txt](LICENSE.txt))

## Contributing

We welcome contributions to Nettest development! Please read our [contributing guidelines](CONTRIBUTING.md).

## Documentation

- [RMBT Protocol Specification](https://www.netztest.at/doc/)
---

**Nettest** - Your reliable tool for network speed measurement! 🚀
