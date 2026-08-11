# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2026-08-11

### Added
- `SerializableGrid` snapshot with `new`, `len`, `is_empty`, `set`, `get`, `iter`, and
  `from_grid`.
- `GridState` implementation for `SerializableGrid`.
- MessagePack serialization (`to_msgpack` / `from_msgpack`) via `rmp-serde`.
- Compact JSON serialization (`to_json` / `from_json`).
- Round-trip tests for MessagePack, JSON, and empty-cell dropping.
