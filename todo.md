# TPT Lattice — Build Checklist

> **Stack:** Rust workspace · WebAssembly · SolidJS · CRDTs · Canvas/WebGL
> **License:** MIT OR Apache-2.0
> **Status (2026-08-12):** The Rust workspace compiles, all non-wasm crates pass `cargo test`, the wasm
> crate builds to `wasm32-unknown-unknown` and evaluates end-to-end (`B1 == 42`), CI is in place, the
> `tpt-lattice-import-xlsx` crate has real-fixture tests, the **Phase 4 SolidJS + Canvas frontend
> builds and bundles** (`npm run build` → `dist/` with the engine worker + wasm asset), the **Phase 3
> sync layer is implemented and tested** (TypeScript `SyncClient`, IndexedDB `OpLog` offline queue,
> reconnect replay with server history reconciliation, and a convergence test), and **merged cells +
> basic styles import** is implemented. As of 2026-08-12 the **Phase 9 UI/UX work is largely done**
> (undo/redo, copy/paste, right-click context menu, column/row resize, find/replace, real cell
> formatting, header row/column selection, Home/End/PageUp/PageDown/Ctrl+Arrow keyboard nav,
> row/column insert/delete wired to the CRDT ops, and a remote-change conflict UI); freeze panes,
> true multi-sheet, and presence cursors remain deferred pending engine/server work. Remaining work:
> crates.io publication and the `v0.1.0` tag/release
> (both require a crates.io token / push, which are auth-gated), plus a large backlog captured below
> from a full platform review (2026-08-12) — critical correctness/security bugs, formula-language and
> UI/UX gaps, accessibility, import/export, CI, adoption/onboarding, and innovative-feature ideas.

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
- [x] Implement `tpt-lattice-sync-client` TypeScript module
- [x] Implement IndexedDB-backed offline op queue (ops persist across page reloads)
- [x] Implement op replay on reconnect; server reconciles diverged histories
- [x] End-to-end convergence test: two browser tabs edit offline, sync, reach identical state

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
- [x] Map merged cells and basic styles

### Publication & Docs
- [x] Write `cargo doc`-quality documentation for all public APIs
- [x] Write per-crate `README.md` with usage examples (all 8 crates + root)
- [x] Write per-crate `CHANGELOG.md` (all 8 crates + root)
- [x] Set up CI (GitHub Actions): `cargo test`, `cargo clippy`, `wasm-pack test`
- [ ] Publish crates to crates.io in dependency order (core → parser → evaluator → ...) — *packaging validated via `cargo publish --dry-run`; blocked on crates.io token + ordered publish*
- [ ] Tag `v0.1.0` release; publish GitHub release notes — *blocked on push to remote*

---

## Phase 6 — Correctness & Reliability Fixes (Critical)

> Found in a full platform review (2026-08-12). These affect data correctness or can
> crash/hang the app on ordinary-looking input.

- [x] Fix CRDT actor-id collision: `SyncClient` never passes an `actor` to `engine.init`, so
      every browser session defaults to the wasm engine's hard-coded `actor=1`, which breaks
      deterministic LWW convergence between concurrent peers (ties resolve by merge order,
      not by a real per-peer identity)
- [x] Fix `RANGE` cell-count integer overflow: the `count` guard in `expand_range`
      (duplicated in `dag.rs` and `eval.rs`) can overflow `u64`/`usize` for large
      column×row spans, silently bypassing `MAX_RANGE_CELLS` and looping ~2^64 times
      (worse on wasm32, where `usize` is 32-bit and the threshold is trivially reachable)
- [x] Fix `CellId::try_from_a1` unbounded column-letter accumulator overflow (panics or
      wraps to a corrupted `CellId` on 14+ consecutive letters immediately followed by
      digits, e.g. `AAAAAAAAAAAAAA1`)
- [x] Call `CellValue::sanitize()` on evaluated formula results in `Evaluator::evaluate`
      (currently only `set_value` sanitizes) so `NaN`/`Infinity` from `SQRT(-1)`,
      `POW`/`ROUND` edge cases, or `1e400` literals can't leak into the grid
- [x] Fix circular-reference fallout: cells that depend on a cycle member currently
      evaluate against stale/old values instead of erroring, and are never re-marked dirty
      to self-correct later
- [x] Make structural CRDT ops (`InsertRow`/`DeleteRow`/`InsertColumn`/`DeleteColumn`)
      causally ordered like `SetCell`/`DeleteCell` — concurrent structural edits currently
      diverge across peers depending on local apply order
- [x] Connect row/column ULID identity lists to actual cell storage — insert/delete
      row/column is currently inert (cells never shift), and delete never cascades or
      tombstones the cells that lived at the deleted position
- [x] Replace the hardcoded `0..1024 x 0..1024` rescan in `crdt_cells()` (wasm) and
      `grid_snapshot()` (`tpt-lattice-io`) with an approach that doesn't silently drop data
      outside that window and doesn't re-scan ~1M cells on every mutation
- [x] Add recovery from wasm-engine mutex poisoning — a panic while the lock is held
      currently bricks the engine permanently until a full page reload
- [x] Call `DependencyGraph::remove` when a cell is cleared, or otherwise prune dead graph
      nodes — currently every cell ever touched permanently occupies a graph node

## Phase 7 — Security & Server Hardening

- [x] Add authentication/authorization to the `/ws` endpoint (currently open to anyone who
      can reach the port)
- [x] Add CORS/Origin checks on the WebSocket upgrade; support `wss://` (currently
      hardcoded plaintext `ws://127.0.0.1:8080/ws` with no origin validation)
- [x] Add a rooms/documents concept — the server currently has one global broadcast channel
      and one history vec for the entire process, so all clients share one document
- [x] Add rate limiting and per-message/per-connection size limits
- [x] Validate inbound messages as real `Op`s, not just "is this JSON" — malformed/garbage
      payloads currently get stored in history forever and rebroadcast to every peer
- [x] Add durable persistence for server history (currently pure in-memory; a restart or
      crash loses every document irrecoverably, contradicting the persistence design in
      `spec.txt`)
- [x] Add snapshot/compaction for op history so reconnect cost doesn't grow unboundedly
      with a document's lifetime edit count
- [x] Attach `.catch()` handling in `SyncClient.onMessage`'s `applyOps` call so a malformed
      remote op doesn't produce an unhandled promise rejection on every connected client

## Phase 8 — Formula Language Gaps (LES)

- [x] Add lookup functions: `VLOOKUP`/`HLOOKUP`/`INDEX`/`XLOOKUP` (a separate `MATCH`-style
      *lookup* function is intentionally **not** added — `MATCH` is already the Ok/Err
      error-handling construct in LES, so a same-named lookup would collide; `XLOOKUP` covers
      the lookup-and-return case. Implemented in `tpt-lattice-evaluator/src/eval.rs`.)
- [x] Add conditional aggregates: `SUMIF`/`COUNTIF`/`AVERAGEIF`/`SUMIFS` (criterion supports
      `>`,`<`,`>=`,`<=`,`<>`,`=` operators and exact matches; `AVERAGEIF`/`SUMIFS` return
      `#DIV/0!`-style errors when no rows match)
- [x] Add `IFERROR`/`IFNA` convenience wrappers
- [x] Add predicates: `ISBLANK`/`ISERROR`/`ISNUMBER`/`ISTEXT`/`ISNA` (added `LatticeError::NA`
      for `#N/A` semantics, recognized by `ISNA`)
- [x] Add string functions: `UPPER`/`LOWER`/`TRIM`/`LEFT`/`RIGHT`/`MID`/`FIND`/`SUBSTITUTE`/
      `REPLACE` and a `&` string-concatenation operator (added `BinaryOp::Concat` to the AST +
      parser). `SPLIT` is **deferred** — LES has no list/array value type to return into yet.
- [ ] Add a `CellValue::Date`/time type plus `DATE`/`TODAY`/`NOW`/`YEAR`/`MONTH`/`DAY`/
      `DATEDIF` — *deferred*: requires adding a `Date` variant to `CellValue` (touches core,
      `tpt-lattice-io`, wasm glue, and the frontend renderer)
- [x] Add statistics functions: `MEDIAN`/`STDEV`/`VAR`/`MODE`/`RANK`/`PERCENTILE`
- [ ] Add named ranges / reusable formulas (resolve the parser ambiguity where any
      `[A-Za-z]+[0-9]+`-shaped identifier is always parsed as a `CellRef` first) — *deferred*
- [ ] Add absolute reference syntax (`$A$1`) and fill/copy semantics — *deferred*
- [ ] Add multi-sheet / 3D references (`Sheet1!A1`) at the core/parser/evaluator level —
      *deferred* (depends on real multi-sheet support in Phase 9)
- [ ] Switch error display to familiar Excel-style codes (`#DIV/0!`, `#REF!`, `#VALUE!`,
      `#NAME?`) instead of prose messages — *partial*: added `LatticeError::NA` (`#N/A`); full
      recoding of all error variants to Excel codes is deferred (touches display + serialization)
- [ ] Make `SUM`/aggregate functions error on non-numeric args instead of silently skipping
      them, for consistency with LES's strict-typing philosophy — *deferred* (would change
      existing `SUM`/`AVERAGE` semantics and break current round-trip tests)
- [x] Support 2-argument `IF` (implicit empty/`FALSE` else-branch)
- [x] Fix the misleading "RANGE used outside of a function argument" error: it now returns a
      clear `ArgumentError` naming `SUM`/`INDEX`/`VLOOKUP` as valid consumers, and every
      range-accepting function (`SUM`, `INDEX`, `VLOOKUP`/`HLOOKUP`, `XLOOKUP`, conditional
      aggregates, statistics) expands the range itself instead of letting it reach the generic
      evaluator
- [x] De-duplicate `expand_range`: the single canonical implementation now lives in
      `dag.rs` (`pub fn expand_range`) and is reused by the dependency collector (`lib.rs`) and
      the evaluator's aggregate/lookup functions (`eval.rs`)

## Phase 9 — Missing UI/UX Features

- [x] Undo/redo (snapshot-based command history; Ctrl+Z / Ctrl+Shift+Z)
- [x] Copy/paste (clipboard integration; TSV over selection)
- [x] Right-click context menu (copy / paste / clear / insert / delete row & column)
- [x] Column/row resize (variable geometry in `grid/metrics.ts`, drag the header borders)
- [ ] Freeze panes — *deferred* (rendering frozen rows/cols not yet implemented)
- [x] Find/replace (dialog: find, next, replace, replace-all over loaded cells)
- [x] Make cell formatting real — Toolbar bold/italic/number-format/alignment now apply a
      per-cell `CellStyle` that the renderer honors (number formatting, alignment, weight/style)
- [ ] Make multi-sheet support real — `SheetTabs` is still a local-UI-only stub; true
      multi-sheet needs engine/server-side multi-`LatticeEngine` support (not yet built)
- [x] Add row/column insert/delete UI, wired to the CRDT ops (`InsertRow`/`DeleteRow`/
      `InsertColumn`/`DeleteColumn` engine requests added in `tpt-lattice-wasm`)
- [x] Fix formula bar to commit on blur (now commits on blur as well as Enter)
- [x] Add keyboard nav: Home/End/PageUp/PageDown/Ctrl+Arrow jump-to-edge
- [x] Add header-click row/column selection (click + drag to select whole rows/columns)
- [ ] Add presence: show other users' cursors/selections — *deferred* (needs server-side
      cursor/selection broadcast; not in the current `/ws` protocol)
- [x] Add conflict UI: surface "this cell changed remotely" — remote `SetCell`/`DeleteCell`
      ops highlight the affected cell (amber border) for a few seconds instead of silently
      repainting

## Phase 10 — Accessibility

- [x] Add ARIA grid semantics — `grid/AccessibleGrid.tsx` mirrors the canvas into a
      visually-hidden `role="grid"`/`role="row"`/`role="gridcell"` tree; the canvas
      container carries `role="grid"` + `aria-activedescendant` pointing at the active cell
- [x] Add a live region (`aria-live="polite"`) announcing active-cell address + value/formula
- [x] Add non-color error indicators — a warning glyph is drawn on error cells in
      `grid/renderer.ts` (red background alone is no longer the only cue)
- [x] Document and test a full keyboard-only workflow — navigation extracted to a pure,
      unit-tested `grid/keyboard.ts` (`keyboard.test.ts`); shortcuts documented in
      `frontend/README.md`

## Phase 11 — Import/Export

- [x] Add `.xlsx` export — new `tpt-lattice-export-xlsx` crate writes OOXML via `zip`
      (no heavy XML dep); round-trip-tested against the importer's structures
- [x] Translate supported Excel formulas into LES on import — `translate_excel_to_les`
      rewrites ranges (`A1:B2` → `RANGE(A1,B2)`) and carries the result in a new
      `ImportedSheet.formulas` map; unsupported constructs (`@`, `!`, `{`) still surface as
      `UnsupportedFormula` (best-effort; LES/Excel function-name divergences are not auto-fixed)
- [x] Add an "import all sheets" helper — `import_all_sheets(bytes)` returns
      `(name, ImportedSheet)` for every sheet in workbook order
- [ ] Preserve a real date type on import instead of flattening `Data::DateTime`/
      `DateTimeIso` to `CellValue::Text` — *deferred*: `CellValue::Date(f64)` now exists, but
      mapping an ISO `DateTime` to an Excel serial `f64` is non-trivial; left as a follow-up
- [x] Extend style import beyond bold/italic/alignment/number-format to fonts, colors,
      borders, and fills — `CellStyle` now carries `fill_color`/`font_color`/`font_name`/`border`
      (ARGB/`borderId` parsed from `styles.xml`)
- [x] Add a version/schema field to `tpt-lattice-io`'s `SerializableGrid` — `format_version`
      (`FORMAT_VERSION`) with `#[serde(default)]` so older files still deserialize
- [x] Persist formulas (not just computed values) in the io format — `SerializableGrid` now has
      a `formulas: BTreeMap<cell, String>` with `set_formula`/`get_formula`/`iter_formulas`
      (round-trip tested). Engine-side population on snapshot remains a follow-up.

> **Phase 12 (CI/Tooling) verified complete 2026-08-12:** all six items were already implemented
> and pass locally (frontend lint/typecheck/4 vitest tests; `wasm-pack test` runs real tests; audit,
> Dependabot, cross-platform matrix, and tag-triggered release automation all present). The checklist
> had drifted from the repo — see the Phase 12 section for detail.

## Phase 12 — CI/Tooling

> **Verified complete (2026-08-12).** All six items below were already implemented in the
> repo and pass locally: `.github/workflows/ci.yml` has `test` (matrix `ubuntu/macos/windows`),
> `lint`, `audit` (cargo + npm), `wasm` (`wasm-pack build` **and** `wasm-pack test --node`), and
> `frontend` (npm ci → lint → typecheck → test → build) jobs; `.github/dependabot.yml` covers
> cargo + npm; `.github/workflows/release.yml` does tag-triggered ordered crates.io publish +
> GitHub Release; and the frontend has `eslint.config.js` + `.prettierrc.json` wired to
> `npm run lint`/`format` (verified: lint, `tsc --noEmit`, and `vitest` all pass).

- [x] Add a frontend CI job: `npm install`, `typecheck`, `test` (vitest), `build` — implemented
      as the `frontend` job in `ci.yml` (verified locally: lint/typecheck/4 vitest tests pass)
- [x] Make CI actually run `wasm-pack test` — the `wasm` job runs `wasm-pack test
      crates/tpt-lattice-wasm --node`, and `crates/tpt-lattice-wasm/src/lib.rs` has real
      `#[wasm_bindgen_test]` tests (`engine_evaluates_through_handle`, `evaluator_core_math`)
- [x] Add `cargo audit`/`npm audit`/Dependabot — `audit` job in `ci.yml` + `.github/dependabot.yml`
      (cargo + npm, weekly)
- [x] Add tag-triggered release automation: ordered crates.io publish + GitHub Release notes —
      `.github/workflows/release.yml` (tag `v*` → publish in dependency order → GitHub Release)
- [x] Add a cross-platform CI matrix — `test` job already runs `ubuntu-latest`/`macos-latest`/
      `windows-latest`
- [x] Add ESLint/Prettier config for the frontend — `eslint.config.js` (typescript-eslint +
      prettier) and `.prettierrc.json` wired to `npm run lint`/`format`

## Phase 13 — Adoption: Examples, Templates & Onboarding

- [x] Add a root-level quick start for running the *full* collaborative app (frontend +
      wasm build + server) — `README.md` now has a "Quick start (full collaborative app)"
      section with one-command and manual paths
- [x] Wire the wasm build into the frontend's `npm run dev`/`build` scripts (e.g.
      `predev`/`prebuild`) so setup is one command instead of a manual multi-step process
      — *already wired* in `frontend/package.json` (`predev`/`prebuild` run `wasm-pack`)
- [x] Add a Docker Compose file or devcontainer for one-command environment setup —
      `Dockerfile`, `Dockerfile.server`, `docker-compose.yml`, `.devcontainer/devcontainer.json`, `.dockerignore`
- [x] Add sample/template spreadsheets (e.g. budget, project tracker) loadable from the UI —
      `examples/templates/*.json` + `examples/README.md`; loadable via Toolbar → Open
- [x] Add a `CONTRIBUTING.md`
- [ ] Add a hosted live demo / CI-published preview build — *deferred*: requires a hosting
      target / CI publish job (auth + infra outside this pass)
- [x] Add an in-app LES formula cheat-sheet/reference — `components/FormulaHelp.tsx` modal,
      opened from Toolbar → Help

## Innovative additions (new ideas)

- [ ] Time-travel / version-history UI built on the existing CRDT op log (scrub back through
      edit history, restore a prior state)
- [ ] Git-style diff/merge view between two sheet versions
- [ ] "What-if" branching: fork a sheet, experiment, merge back
- [ ] AI-assisted formula authoring: natural-language-to-LES translation, hover
      explain/lint on formulas
- [ ] Sandboxed user-defined functions via a wasm plugin model for power users
- [ ] In-app formula unit tests: let users assert expected values for cells/ranges as a
      "check sheet" action
- [ ] Dependency-graph visualizer surfacing the DAG that already exists in
      `tpt-lattice-evaluator`, to help users understand/debug complex sheets
- [ ] Community template gallery/marketplace for shared sheet templates
