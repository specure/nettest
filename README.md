# 🌐 Nettest - Network Speed Measurement

![Network Speed Measurement](Gemini_Generated_Image_skkcnfskkcnfskkc.png)

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

### Run Client

```bash
# TCP client
nettest -c <SERVER_ADDRESS>

# WebSocket client
nettest -c <SERVER_ADDRESS> -ws

# TLS client 
nettest -c <SERVER_ADDRESS> -tls
```

## ⚙️ Configuration

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
- **Latency** - Network response time

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
