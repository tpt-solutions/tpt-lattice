# TPT Lattice — Build Checklist

> **Stack:** Rust workspace · WebAssembly · SolidJS · CRDTs · Canvas/WebGL
> **License:** MIT OR Apache-2.0
> **Status (2026-08-12):** The Rust workspace compiles, all non-wasm crates pass `cargo test`, the wasm
> crate builds to `wasm32-unknown-unknown` and evaluates end-to-end (`B1 == 42`), CI is in place, the
> `tpt-lattice-import-xlsx` crate has real-fixture tests, and the **Phase 4 SolidJS + Canvas frontend
> builds and bundles** (`npm run build` → `dist/` with the engine worker + wasm asset). Remaining work:
> the TypeScript sync client, IndexedDB offline queue, merged-cells/styles import, crates.io publication,
> and the `v0.1.0` tag/release.

---

## Phase 1 — Headless Core (Weeks 1–4)

### Project Setup
- [x] Initialize git repository
- [x] Create root `Cargo.toml` workspace (list all member crates)
- [x] Add `.gitignore` (Rust + Node + wasm artifacts)
- [x] Add root `README.md` with project overview
- [x] Add `LICENSE-MIT` and `LICENSE-APACHE` files

### `tpt-lattice-core` crate
- [x] Scaffold crate with `#![no_std]` + `extern crate alloc`
- [x] Implement `CellId` as a compact `u64` bitfield (20-bit column, 44-bit row)
- [x] Implement `CellValue` enum (`Empty`, `Number(f64)`, `Text(String)`, `Boolean(bool)`, `Error(LatticeError)`)
- [x] Define `LatticeError` error type hierarchy
- [x] Define `GridState` trait (read/write cell interface)
- [x] Feature-gate `serde` support (`Serialize` / `Deserialize` impls)
- [x] Write unit tests for `CellId` encoding/decoding round-trips

### `tpt-lattice-parser` crate
- [x] Scaffold crate with `#![no_std]` + `nom` dependency
- [x] Define AST node types (literals, binary ops, function calls, ranges, error handlers)
- [x] Implement lexer (tokenize formula strings)
- [x] Implement parser (tokens → typed AST)
- [x] Handle LES-specific syntax: `RANGE()`, `Ok`/`Err` pattern matching, explicit casts
- [x] Write `proptest` property-based tests for parser round-trips
- [x] Write unit tests for known-good and known-error formula strings

---

## Phase 2 — Evaluator & DAG (Weeks 5–8)

### `tpt-lattice-evaluator` crate
- [x] Scaffold crate (depends on `core` + `parser` + `petgraph`)
- [x] Build `DependencyGraph` struct: sparse directed graph of `CellId` nodes
- [x] Implement dependency tracking: `dependents` (who needs me) and `dependencies` (who I need)
- [x] Implement cycle detection via Tarjan's Strongly Connected Components (SCC)
- [x] Implement DAG-walk evaluator: AST node → `CellValue` with `GridState`
- [x] Implement dirty-cell invalidation and incremental re-evaluation
- [x] Write tests: formula chains, circular reference detection, cascading updates

### `tpt-lattice-wasm` crate
- [x] Scaffold crate with `wasm-bindgen` (`cdylib` + `rlib`) build config
- [x] Expose engine API to JS: `setCell`, `getCell`, `evaluate`, `applyOps`, `reset` (JSON envelope)
- [x] Design Web Worker message protocol (JSON request/response envelope)
- [x] Compile to Wasm with `wasm-pack build` (`cargo build --target wasm32-unknown-unknown` succeeds; `pkg/` generated via `wasm-bindgen`)
- [x] Write a minimal HTML/JS harness to prove evaluation works end-to-end in browser (`www/`, verified via Node smoke test: `B1 == 42`)

---

## Phase 3 — CRDT & Sync (Weeks 9–12)

### `tpt-lattice-crdt` crate
- [x] Scaffold crate (depends on `core` + `serde`)
- [x] Define `Op` enum: `SetCell`, `DeleteCell`, `InsertRow`, `InsertColumn`, `DeleteRow`, `DeleteColumn`
- [x] Implement vector clock (`VectorClock`) for causal ordering
- [x] Assign immutable ULID/UUID identifiers to rows and columns (not integer indices)
- [x] Implement CRDT merge: commutative, associative op application (deterministic LWW)
- [x] Integrate with evaluator: op application → DAG invalidation → re-evaluation (via wasm glue)

### Sync Infrastructure
- [x] Build minimal Axum WebSocket server (`tpt-lattice-server`): broadcast ops to all peers
- [ ] Implement `tpt-lattice-sync-client` TypeScript module
- [ ] Implement IndexedDB-backed offline op queue (ops persist across page reloads)
- [ ] Implement op replay on reconnect; server reconciles diverged histories
- [ ] End-to-end convergence test: two browser tabs edit offline, sync, reach identical state

---

## Phase 4 — Canvas UI (Weeks 13–16)

### Frontend Scaffold
- [x] Initialize SolidJS project with Vite + TypeScript (`npm create solid`)
- [x] Configure `vite-plugin-wasm` and Web Worker support (Vite + `vite-plugin-solid`, ES-module worker)
- [x] Define TypeScript message types for main thread ↔ Wasm worker protocol (`src/types.ts`)
- [x] Instantiate Wasm worker; route all engine calls through it (`engine/engineClient.ts`)

### Grid Renderer
- [x] Implement virtualized Canvas renderer: only draw visible viewport + buffer rows/cols (`grid/renderer.ts`, `grid/metrics.ts`)
- [x] Render grid lines, cell backgrounds, and cell text (numbers, strings, booleans, errors)
- [x] Implement smooth scroll (wheel → scroll signals → redraw; auto-scroll active cell into view)
- [x] Handle window resize (ResizeObserver) and high-DPI (`devicePixelRatio`) scaling
- [x] Implement cell selection (single cell, range drag/shift) with highlight overlay

### Cell Editing
- [x] Implement DOM overlay: absolutely-positioned `<textarea>` snapped over active canvas cell (`grid/CellEditor.tsx`)
- [x] Handle formula entry (starts with `=`) vs plain value entry
- [x] Commit on Enter / Tab; cancel on Escape
- [x] Display `LatticeError` values as styled error chips in cells (red background + `#Error` text)

### UI Chrome (SolidJS)
- [x] Formula bar: shows active cell address + raw formula/value
- [x] Sheet tab strip (add / rename / delete sheets)
- [x] Toolbar: bold, italic, number format, alignment (stub — no evaluator impact yet)
- [x] Connect all UI interactions to Wasm worker via message protocol

The frontend builds and bundles end-to-end: `npm install` → `npm run build` produces `dist/`
with the engine worker and `tpt_lattice_wasm_bg.wasm` asset. Run `npm run dev` to use it
(requires building `crates/tpt-lattice-wasm/pkg` first — see `frontend/README.md`).

---

## Phase 5 — Polish & Ingest (Weeks 17+)

### `tpt-lattice-io` crate
- [x] Scaffold crate (depends on `core` + `rmp-serde`)
- [x] Implement `GridState` → MessagePack serialization
- [x] Implement MessagePack → `GridState` deserialization
- [x] Implement compact JSON export/import as an alternative format
- [x] Write round-trip tests (serialize → deserialize → assert grid equality)

### `tpt-lattice-import-xlsx` crate
- [x] Scaffold crate (feature-gated, depends on `core` + `calamine`)
- [x] Map `.xlsx` cell types to `CellValue` variants
- [x] Map unsupported Excel formulas → `Error(LatticeError::UnsupportedFormula)`
- [x] Map named ranges (best-effort via `defined_names`)
- [x] Write import tests against real `.xlsx` fixture files (`tests/fixtures/sample.xlsx` + `tests/import.rs`)
- [ ] Map merged cells and basic styles

### Publication & Docs
- [x] Write `cargo doc`-quality documentation for all public APIs
- [x] Write per-crate `README.md` with usage examples (all 8 crates + root)
- [x] Write per-crate `CHANGELOG.md` (all 8 crates + root)
- [x] Set up CI (GitHub Actions): `cargo test`, `cargo clippy`, `wasm-pack test`
- [ ] Publish crates to crates.io in dependency order (core → parser → evaluator → ...)
- [ ] Tag `v0.1.0` release; publish GitHub release notes
