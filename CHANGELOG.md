# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2026-08-11

Initial workspace release of the TPT Lattice spreadsheet engine.

### Added
- Workspace with eight crates:
  - `tpt-lattice-core` — `CellId`, `CellValue`, `LatticeError`, `GridState` (`no_std`).
  - `tpt-lattice-parser` — LES lexer/parser and AST (`no_std`).
  - `tpt-lattice-evaluator` — dependency DAG, Tarjan cycle detection, incremental evaluation.
  - `tpt-lattice-crdt` — operation-based CRDT with vector clocks and ULID row/column ids.
  - `tpt-lattice-io` — MessagePack and JSON grid serialization.
  - `tpt-lattice-import-xlsx` — opt-in `.xlsx` import via calamine.
  - `tpt-lattice-wasm` — `wasm-bindgen` worker API.
  - `tpt-lattice-server` — Axum WebSocket op-broadcast server.
- Per-crate `README.md`, `CHANGELOG.md`, distinct `keywords`, and `categories`.
- Root `README.md`, `LICENSE-MIT`, and `LICENSE-APACHE`.

### Notes
- The collaborative frontend (SolidJS UI), TypeScript sync client, IndexedDB offline queue,
  CI, and crates.io publication are tracked in `todo.md` and land in later releases.
