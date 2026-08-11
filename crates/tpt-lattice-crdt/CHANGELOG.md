# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2026-08-11

### Added
- `Op` enum: `SetCell`, `DeleteCell`, `InsertRow`, `InsertColumn`, `DeleteRow`, `DeleteColumn`.
- `VectorClock` version vector with `tick`, `merge`, `happens_before`, and `concurrent`.
- `ActorId` type alias and deterministic last-writer-wins `precedence`.
- `CrdtStore` with `new`, `actor`, `clock`, `set_cell`, `delete_cell`, `insert_row`,
  `apply`, `merge_ops`, `get_cell`, `row_count`, `column_count`.
- Immutable `ulid::Ulid` identifiers for rows and columns.
- Tests: vector-clock ordering, concurrent clocks, offline convergence, delete-after-set,
  and ULID row insertion.
