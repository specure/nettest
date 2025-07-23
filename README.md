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

# Static build for Linux
cargo build --release --target x86_64-unknown-linux-musl
```

#### Docker-based Cross-compilation

For maximum compatibility with older Linux distributions, use Docker-based cross-compilation:

```bash
# Build Docker image
docker build -f Dockerfile.build -t nettest-builder .

# Run builds for all architectures
docker run --rm -v $(pwd):/app -w /app nettest-builder /usr/local/bin/build.sh
```

This will create static musl binaries for:
- x86_64 (64-bit Intel/AMD)
- aarch64 (64-bit ARM)
- i686 (32-bit Intel)
- armv7 (32-bit ARM)

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
