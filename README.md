# RMBT Measurement server 2.0

> ⚠️ **Note:** This server is in the testing phase.


## Overview

This is a Rust implementation of the RMBT (RTR Multithreaded Broadband Test) server, designed for conducting network measurements based on the RMBT protocol. The server supports both direct TCP socket connections and WebSocket protocol communications.

## Features

- Full RMBT protocol implementation
- Support for both TCP and WebSocket connections
- Configurable worker threads
- TLS/SSL support
- Token-based authentication
- Comprehensive test suite

## Prerequisites

Required packages:
- Rust toolchain (latest stable version)

## Build

Build the server using Cargo:

```bash
# Debug build
cargo build

# Release build with optimizations
cargo build --release

# Release static build with optimizations
cargo build --release --target x86_64-unknown-linux-musl
```

The release build will be available at `target/release/rmbt_server`.

## Configuration

### Secret Keys
The server uses authentication keys as specified in the RMBT protocol. Keys are stored in `secret.key`:
- One key per line
- Format: `<key> <label>`
- Label is logged to syslog when a client uses the key

### Server Parameters

```
Usage: rmbtd [OPTIONS]

Options:
  -l, -L <address>    Listen address and port (SSL with -L)
                      Format: "port" or "ip:port" or "[ipv6]:port"
                      Examples: 
                      - "443"
                      - "1.2.3.4:1234"
                      - "[2001:1234::567A]:1234"
                      Can be specified multiple times

  -c <path>          Path to SSL certificate (PEM format)
                     Include intermediate certificates if needed

  -k <path>          Path to SSL key file (PEM format)

  -t <number>        Number of worker threads (default: 200)

  -u <user>          Drop privileges to specified user
                     Requires root privileges

  -d                 Run as daemon in background

  -D                 Enable debug logging

  -w                 Enable HTTP/WebSocket mode

  -v <version>       Serve old clients (example: "0.3")
```

## Protocol Support

### HTTP/WebSocket Mode (-w)
- Supports HTTP GET requests with connection upgrades
- Upgrades to either RMBT or WebSocket protocol
- Follows RFC 2616 for upgrades

#### RMBT Upgrade
Request:
```
GET /rmbt HTTP/1.1
Connection: Upgrade
Upgrade: RMBT
RMBT-Version: 1.3.0
```

#### WebSocket Upgrade
Request:
```
GET /rmbt HTTP/1.1
Connection: Upgrade
Upgrade: websocket
Sec-WebSocket-Version: 13
Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==
```

### Legacy Mode
Direct TCP socket communication without HTTP wrapper.

## Testing

Run the test suite:
```bash
cargo test
```

For specific test categories:
```bash
cargo test --test basic_server  # Basic server tests
cargo test --test handler      # Protocol handler tests
```

## Performance Testing

Performance test results and system metrics are saved in the `performance/` directory.

## Documentation

- [RMBT Protocol Specification](https://www.netztest.at/doc/)
- [RTR-Netztest](https://www.netztest.at)

## Related Projects

- [RMBTws Client](https://github.com/rtr-nettest/rmbtws)
- [RTR-Netztest/open-rmbt](https://github.com/rtr-nettest/open-rmbt)
- [RMBT C client](https://github.com/lwimmer/rmbt-client)

## License

- Source code: Apache License ([LICENSE.txt](LICENSE.txt))
