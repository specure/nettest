# Bonjour/mDNS Client Example for Nettest

This is a working TypeScript example demonstrating how to discover nettest servers in the local network using Bonjour/mDNS service discovery.

## What it does

This client:
- Browses for `_nettest._tcp` services in the local network
- Parses TXT records to extract server configuration (ports, version, etc.)
- Displays discovered servers and their properties
- Provides a foundation for connecting to discovered servers

## Prerequisites

- Node.js (v14 or higher)
- npm or yarn

## Installation

First, install all dependencies:

```bash
cd bonjour-example
npm install
```

**Note**: If you see TypeScript errors about missing types, they will be resolved after running `npm install`, which installs `@types/node` and `@types/bonjour`.

## Usage

### Development mode (with ts-node)

```bash
npm run dev
```

### Build and run

```bash
npm run build
npm start
```

### Watch mode (auto-rebuild on changes)

```bash
npm run watch
```

## How it works

1. **Service Discovery**: The client uses the `bonjour` library to browse for services of type `_nettest._tcp` in the local network.

2. **Service Announcement**: When your Rust server starts the mDNS service (via `start_mdns_service`), it announces itself with:
   - Service type: `_nettest._tcp.local.`
   - Instance name: `nettest.local`
   - TXT records containing server configuration

3. **Client Discovery**: The TypeScript client receives mDNS announcements and:
   - Extracts service information (hostname, port, IP addresses)
   - Parses TXT records to get configuration (tcp_port, tls_port, version, etc.)
   - Displays the information and can connect to the server

## Example Output

```
🔍 Starting mDNS/Bonjour service discovery...
Looking for services: _nettest._tcp.local

Discovery is running. Press Ctrl+C to stop.

✅ Service discovered:
   Name: nettest
   Type: nettest._tcp.local
   Host: nettest.local
   Port: 8080
   Addresses: 192.168.1.100, fe80::1
   TXT Records:
     tcp_port: 8080
     tls_port: 8443
     version: 1.0.0

📋 Server Configuration:
   TCP Port: 8080
   TLS Port: 8443
   Version: 1.0.0
   Server Name: N/A

🔗 Connection URL: tcp://nettest.local:8080

💡 Example: Ready to connect to server at nettest.local:8080

────────────────────────────────────────────────────────────
```

## Understanding the mDNS Query

When you see a log like:
```
11:52:54.662  Add        3   1 local.               _nettest._tcp.       nettest ?
```

This represents:
- **Add**: Operation (adding a record to cache after receiving response)
- **3**: DNS record type (PTR - Pointer record for service discovery)
- **1**: DNS class (IN - Internet)
- **local.**: mDNS domain
- **_nettest._tcp.**: Service type
- **nettest**: Service instance name
- **?**: Query indicator

The client sends a multicast DNS query to `224.0.0.251:5353` asking for `_nettest._tcp.local.` services, and your server responds with:
- PTR record: `_nettest._tcp.local. IN PTR nettest._nettest._tcp.local.`
- SRV record: `nettest._nettest._tcp.local. IN SRV 0 0 <port> <hostname>`
- TXT record: `nettest._nettest._tcp.local. IN TXT "tcp_port=..." "version=..."`

## Customization

You can extend the `connectToServer` method in `src/index.ts` to implement your actual connection logic, such as:
- Opening a TCP socket
- Establishing a WebSocket connection
- Making HTTP requests
- Using the server configuration from TXT records

## Troubleshooting

- **No services found**: 
  - Make sure your Rust server is running and has mDNS service enabled
  - Try using `dns-sd -B _nettest._tcp` to verify services are discoverable
  - The `bonjour` library may have limitations on some systems
  
- **Permission errors**: On Linux, you may need to run with appropriate permissions for multicast

- **Firewall issues**: Ensure UDP port 5353 is not blocked

- **Library compatibility**: 
  - The `bonjour` npm library may not work reliably on all systems
  - If `bonjour` doesn't find services but `dns-sd` command does, this may be a library limitation

## License

MIT
