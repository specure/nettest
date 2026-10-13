# Changelog

All notable changes to nettest are recorded in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and
nettest follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

The release notes of every release contain the section of this file that belongs
to that version. Releases before 2.1.0 are listed on the
[releases page](https://github.com/specure/nettest/releases) only.

## [Unreleased]

### Added

- A browser speed test built into the server. `nettest -s` serves the page on
  the measurement port, so any device on the network can open
  `http://<server>:5005` and measure without installing anything and without
  TLS certificates. The page connects to the server it was loaded from and
  starts the measurement on its own once it is visible; `?autostart=0` turns
  that off.
  - The page runs the WASM build of the nettest client, so browser and command
    line results come from the same code.
  - Download and upload are drawn as one ring of live samples, and ping as one
    dot per round trip.
  - Results show the change since the previous run and a plain sentence about
    what the connection is good for. Past runs are kept in the browser.
  - Every client option can be set in the page or through URL parameters.
  - Results can be copied as text, and shared as an image where the browser
    supports sharing files. Elsewhere the image is downloaded.
  - The page, including the WASM client, adds about 160 KB (gzip) to the
    binary. It needs no npm or framework.
- The `enable_web_ui` setting in `nettest.conf`. The browser speed test is on
  by default; `enable_web_ui = false` turns it off.
- On startup the server prints the addresses to open on other devices, the
  local network address first, and a QR code for phones when it runs in a
  terminal.
- `scripts/build-web.sh` builds the browser page into `dist/`.
  `scripts/build-web.sh --check` verifies, without building, that `dist/` was
  built from the current `web/` files and WASM client. CI runs the check.

### Changed

- mDNS is on by default and is now controlled by `enable_mdns` in
  `nettest.conf`. Set `enable_mdns = false` to turn it off.
- The browser page is served on the measurement port instead of TCP port
  5006, and no longer requires mDNS. The server sends the files compressed with
  gzip, with `application/wasm` for the WASM module, and with ETags.
- The page in `dist/` is a new page without Vue. It replaces the previous Vue
  page and its hardcoded `nettest.local:5005` address.
- Packet loss can only be measured in the browser on HTTPS or `localhost`
  pages with WebTransport. Elsewhere the page leaves the packet loss value and
  its step out instead of showing it as not measured.
- The WASM client reports the round-trip time of each ping in its ping event
  (`samplesMs`).

### Removed

- The `-mdns` option of the server. mDNS is now on by default. **A server
  started with `-mdns` exits with an unknown option error;** remove the option
  from scripts and service files.

### Fixed

- `enable_mdns = true` in `nettest.conf` had no effect on the server.
- The server announced version 2.0.0 over mDNS instead of its actual version.

## [2.1.0] - 2026-08-05

### Added

- The `-json` option. The client writes the measurement to stdout as one JSON
  document. The document reports the native units of nettest: milliseconds for
  latency and jitter, bits per second for speed, bytes for the transferred
  volume and percent for packet loss. A value that nettest did not measure is
  left out instead of being reported as zero.

### Changed

- The client writes its diagnostics to stderr instead of stdout. In `-raw` mode
  stdout now holds the single `ping/download/upload` line only.

### Fixed

- The jitter table and the packet loss table no longer appear in `-raw` mode.
