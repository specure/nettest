# Changelog

All notable changes to nettest are recorded in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and
nettest follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

The release notes of every release contain the section of this file that belongs
to that version. Releases before 2.1.0 are listed on the
[releases page](https://github.com/specure/nettest/releases) only.

## [Unreleased]

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
