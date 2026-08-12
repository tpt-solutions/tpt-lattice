# TPT Lattice

A next-generation, real-time collaborative spreadsheet engine built from first principles.

TPT Lattice is designed for mathematical correctness, memory safety, and absolute
predictability. It decouples the calculation engine from the UI, compiles the core
logic to WebAssembly for near-native browser performance, and uses a Conflict-free
Replicated Data Type (CRDT) for robust, offline-first, real-time collaboration.

Unlike legacy spreadsheets burdened by implicit type coercion, TPT Lattice enforces
**strict typing** and treats **errors as first-class citizens** that propagate safely
(inspired by Rust's `Result`/`Option`).

## Workspace layout

| Crate | Purpose | `no_std`? |
|-------|---------|-----------|
| `tpt-lattice-core` | Foundational types: `CellId`, `CellValue`, `GridState`, errors | ✅ |
| `tpt-lattice-parser` | Lexer + parser for the Lattice Expression Syntax (LES) | ✅ |
| `tpt-lattice-evaluator` | Dependency DAG, cycle detection, incremental evaluation | ❌ |
| `tpt-lattice-crdt` | Operation-based CRDT for conflict-free grid mutations | ❌ |
| `tpt-lattice-io` | MessagePack / compact JSON serialization | ❌ |
| `tpt-lattice-import-xlsx` | Opt-in `.xlsx` → `CellValue` translation (calamine) | ❌ |
| `tpt-lattice-export-xlsx` | Write grids back out to `.xlsx` (OOXML) | ❌ |
| `tpt-lattice-wasm` | `wasm-bindgen` glue exposing the engine to JS | ❌ |
| `tpt-lattice-server` | Minimal Axum WebSocket op-broadcast server | ❌ |

See [`spec.txt`](./spec.txt) for the full design document and
[`todo.md`](./todo.md) for the build roadmap.

## Quick start (headless engine)

```rust
use tpt_lattice_core::{CellId, CellValue};
use tpt_lattice_evaluator::Evaluator;

let mut engine = Evaluator::new();
engine.set_value(CellId::from_a1("A1"), CellValue::Number(21.0));
engine.set_formula(CellId::from_a1("B1"), "=A1 * 2").unwrap();
engine.evaluate().unwrap();
assert_eq!(engine.get_value(CellId::from_a1("B1")), CellValue::Number(42.0));
```

## Quick start (full collaborative app)

The browser UI (`frontend/`) talks to the engine compiled to WebAssembly inside a
Web Worker, and can sync changes over a WebSocket server (`tpt-lattice-server`).

### One-command (recommended)

The frontend's `dev`/`build` scripts automatically build the wasm engine first:

```sh
# Terminal 1 — the collaborative sync server (optional; the UI works single-user without it)
cargo run -p tpt-lattice-server

# Terminal 2 — the UI (builds wasm, then serves on http://localhost:5173)
cd frontend
npm install
npm run dev
```

Then open <http://localhost:5173>. Use **Toolbar → Open** to load a sample grid
from [`examples/templates/`](./examples/templates), and **Help** for the LES
formula reference.

### Manual build

```sh
# 1. Build the wasm engine package
cd crates/tpt-lattice-wasm
wasm-pack build --target web --out-dir pkg

# 2. (Optional) start the sync server
cargo run -p tpt-lattice-server

# 3. Build/serve the frontend
cd frontend
npm install
npm run build      # -> dist/ (engine worker + wasm asset)
# or: npm run dev   # -> http://localhost:5173
```

A `Dockerfile`, `docker-compose.yml`, and a VS Code `.devcontainer` are provided
for a one-command, dependency-free environment — see
[`docker-compose.yml`](./docker-compose.yml).

## Building & testing

```sh
cargo build --workspace
cargo test  --workspace
cargo clippy --workspace --all-targets
```

## The Lattice Expression Syntax (LES)

LES abandons Excel's implicit magic:

- **Strict typing** — `="5" + 5` is a `TypeError`; use `NUMBER("5") + 5`.
- **Explicit ranges** — `SUM(RANGE(A1, B10))` instead of ambiguous `A1:B10`.
- **First-class errors** — `MATCH(A1, Ok(v) => v * 2, Err(e) => 0)`.
- **Deterministic execution** — no volatile functions without explicit opt-in.

## License

Licensed under either of [MIT](./LICENSE-MIT) or
[Apache-2.0](./LICENSE-APACHE) at your option.
