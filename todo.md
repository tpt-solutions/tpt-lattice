# TPT Lattice — Build Checklist

> **Stack:** Rust workspace · WebAssembly · SolidJS · CRDTs · Canvas/WebGL
> **License:** MIT OR Apache-2.0

---

## Phase 1 — Headless Core (Weeks 1–4)

### Project Setup
- [ ] Initialize git repository
- [ ] Create root `Cargo.toml` workspace (list all member crates)
- [ ] Add `.gitignore` (Rust + Node + wasm artifacts)
- [ ] Add root `README.md` with project overview
- [ ] Add `LICENSE-MIT` and `LICENSE-APACHE` files

### `tpt-lattice-core` crate
- [ ] Scaffold crate with `#![no_std]` + `extern crate alloc`
- [ ] Implement `CellId` as a compact `u64` bitfield (20-bit column, 44-bit row)
- [ ] Implement `CellValue` enum (`Empty`, `Number(f64)`, `Text(String)`, `Boolean(bool)`, `Error(LatticeError)`)
- [ ] Define `LatticeError` error type hierarchy
- [ ] Define `GridState` trait (read/write cell interface)
- [ ] Feature-gate `serde` support (`Serialize` / `Deserialize` impls)
- [ ] Write unit tests for `CellId` encoding/decoding round-trips

### `tpt-lattice-parser` crate
- [ ] Scaffold crate with `#![no_std]` + `nom` dependency
- [ ] Define AST node types (literals, binary ops, function calls, ranges, error handlers)
- [ ] Implement lexer (tokenize formula strings)
- [ ] Implement parser (tokens → typed AST)
- [ ] Handle LES-specific syntax: `RANGE()`, `Ok`/`Err` pattern matching, explicit casts
- [ ] Write `proptest` property-based tests for parser round-trips
- [ ] Write unit tests for known-good and known-error formula strings

---

## Phase 2 — Evaluator & DAG (Weeks 5–8)

### `tpt-lattice-evaluator` crate
- [ ] Scaffold crate (depends on `core` + `parser` + `petgraph`)
- [ ] Build `DependencyGraph` struct: sparse directed graph of `CellId` nodes
- [ ] Implement dependency tracking: `dependents` (who needs me) and `dependencies` (who I need)
- [ ] Implement cycle detection via Tarjan's Strongly Connected Components (SCC)
- [ ] Implement DAG-walk evaluator: AST node → `CellValue` with `GridState`
- [ ] Implement dirty-cell invalidation and incremental re-evaluation
- [ ] Write tests: formula chains, circular reference detection, cascading updates

### `tpt-lattice-wasm` crate
- [ ] Scaffold crate with `wasm-bindgen` + `wasm-pack` build config
- [ ] Expose engine API to JS: `setCell`, `getCell`, `evaluateAll`, `loadState`
- [ ] Design Web Worker message protocol (JSON or `postMessage` binary)
- [ ] Compile to Wasm with `wasm-pack build`
- [ ] Write a minimal HTML/JS harness to prove evaluation works end-to-end in browser

---

## Phase 3 — CRDT & Sync (Weeks 9–12)

### `tpt-lattice-crdt` crate
- [ ] Scaffold crate (depends on `core` + `serde`)
- [ ] Define `Op` enum: `SetCell`, `DeleteCell`, `InsertRow`, `InsertColumn`, `DeleteRow`, `DeleteColumn`
- [ ] Implement vector clock (`VectorClock`) for causal ordering
- [ ] Assign immutable ULID/UUID identifiers to rows and columns (not integer indices)
- [ ] Implement CRDT merge: commutative, associative op application
- [ ] Integrate with evaluator: op application → DAG invalidation → re-evaluation

### Sync Infrastructure
- [ ] Build minimal Axum WebSocket server (`tpt-lattice-server`): broadcast ops to all peers
- [ ] Implement `tpt-lattice-sync-client` TypeScript module
- [ ] Implement IndexedDB-backed offline op queue (ops persist across page reloads)
- [ ] Implement op replay on reconnect; server reconciles diverged histories
- [ ] End-to-end convergence test: two browser tabs edit offline, sync, reach identical state

---

## Phase 4 — Canvas UI (Weeks 13–16)

### Frontend Scaffold
- [ ] Initialize SolidJS project with Vite + TypeScript (`npm create solid`)
- [ ] Configure `vite-plugin-wasm` and Web Worker support
- [ ] Define TypeScript message types for main thread ↔ Wasm worker protocol
- [ ] Instantiate Wasm worker; route all engine calls through it

### Grid Renderer
- [ ] Implement virtualized Canvas renderer: only draw visible viewport + buffer rows/cols
- [ ] Render grid lines, cell backgrounds, and cell text (numbers, strings, booleans, errors)
- [ ] Implement smooth scroll with requestAnimationFrame loop
- [ ] Handle window resize and high-DPI (devicePixelRatio) scaling
- [ ] Implement cell selection (single cell, range) with highlight overlay

### Cell Editing
- [ ] Implement DOM overlay: absolutely-positioned `<textarea>` snapped over active canvas cell
- [ ] Handle formula entry (starts with `=`) vs plain value entry
- [ ] Commit on Enter / Tab; cancel on Escape
- [ ] Display `LatticeError` values as styled error chips in cells

### UI Chrome (SolidJS)
- [ ] Formula bar: shows active cell address + raw formula/value
- [ ] Sheet tab strip (add / rename / delete sheets)
- [ ] Toolbar: bold, italic, number format, alignment (stub — no evaluator impact yet)
- [ ] Connect all UI interactions to Wasm worker via message protocol

---

## Phase 5 — Polish & Ingest (Weeks 17+)

### `tpt-lattice-io` crate
- [ ] Scaffold crate (depends on `core` + `rmp-serde`)
- [ ] Implement `GridState` → MessagePack serialization
- [ ] Implement MessagePack → `GridState` deserialization
- [ ] Implement compact JSON export/import as an alternative format
- [ ] Write round-trip tests (serialize → deserialize → assert grid equality)

### `tpt-lattice-import-xlsx` crate
- [ ] Scaffold crate (feature-gated, depends on `core` + `calamine`)
- [ ] Map `.xlsx` cell types to `CellValue` variants
- [ ] Map unsupported Excel formulas → `Error(LatticeError::UnsupportedFormula)`
- [ ] Map named ranges, merged cells, and basic styles
- [ ] Write import tests against real `.xlsx` fixture files

### Publication & Docs
- [ ] Write `cargo doc`-quality documentation for all public APIs
- [ ] Write per-crate `README.md` with usage examples
- [ ] Set up CI (GitHub Actions): `cargo test`, `cargo clippy`, `wasm-pack test`
- [ ] Publish crates to crates.io in dependency order (core → parser → evaluator → ...)
- [ ] Tag `v0.1.0` release; publish GitHub release notes
