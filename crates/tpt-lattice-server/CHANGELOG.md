# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2026-08-11

### Added
- Axum WebSocket server with a `/ws` route (`router`, `serve`, `ws_handler`, `handle_socket`).
- `Hub` broadcast channel type relaying op JSON to all peers.
- Inbound JSON validation before relaying.
- Per-connection task supervision (sender/receiver abort on failure).
- `tpt-lattice-server` binary.
- Test: broadcast hub delivers an op to a subscriber.
