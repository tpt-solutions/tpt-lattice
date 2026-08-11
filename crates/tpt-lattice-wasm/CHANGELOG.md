# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2026-08-11

### Added
- `wasm-bindgen` crate (`cdylib` + `rlib`) exposing the engine to JavaScript.
- JSON `Request` / `Response` envelope protocol for Web Worker messaging.
- `LatticeEngine` handle pairing an `Evaluator` with a `CrdtStore`, with `new` and `handle`.
- `SetCell`, `SetFormula`, `GetCell`, `Evaluate`, `ApplyOps`, and `Reset` request handling.
- `set_cells_json` convenience batch setter.
- CRDT-recorded local edits and evaluator re-materialization on remote op merge.
