# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2026-08-11

### Added
- `CellId`: compact `u64` bitfield (20-bit column, 44-bit row) with A1-style parsing
  (`from_a1`, `try_from_a1`, `FromStr`) and rendering (`to_a1`), plus raw bit round-trips
  (`from_bits` / `to_bits`).
- `CellValue` enum: `Empty`, `Number(f64)`, `Text(String)`, `Boolean(bool)`, `Error(LatticeError)`,
  with downcast helpers (`as_number`, `as_text`, `as_bool`, `as_error`), `sanitize()` for
  non-finite numbers, and `From` conversions.
- `LatticeError` hierarchy with convenience constructors (`type_error`, `name_error`,
  `ref_error`, `argument_error`, `unsupported`, `internal`).
- `GridState` trait: the read/write interface implemented by every storage backend.
- `MAX_COLUMN` / `MAX_ROW` constants.
- Optional `serde` feature providing `Serialize` / `Deserialize` for all public types.
- Round-trip unit tests for `CellId` encoding and `CellValue` sanitization.
