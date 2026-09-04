# Contributing to TPT Lattice

Thanks for your interest in improving TPT Lattice! This document covers the
workflow for the Rust workspace and the SolidJS frontend.

## Project philosophy

TPT Lattice values **correctness, memory safety, and predictability** above
convenience:

- The engine is **strictly typed**. Errors are first-class values that propagate;
  do not paper over type mismatches with implicit coercion.
- The core crates (`tpt-lattice-core`, `tpt-lattice-parser`) are `#![no_std]` where
  possible so they can target wasm and embedded environments.
- Collaborate via a CRDT: concurrent edits must converge deterministically
  (last-writer-wins keyed on a real per-peer `actor` id, never the hardcoded
  default).

## Repository layout

| Area | Crates |
|------|--------|
| Engine | `tpt-lattice-core`, `tpt-lattice-parser`, `tpt-lattice-evaluator` |
| Collaboration | `tpt-lattice-crdt`, `tpt-lattice-server`, `tpt-lattice-wasm` |
| I/O | `tpt-lattice-io`, `tpt-lattice-import-xlsx`, `tpt-lattice-export-xlsx` |
| UI | `frontend/` (SolidJS + Canvas, engine runs in a Web Worker) |

See [`README.md`](./README.md) for a crate-by-crate description and
[`todo.md`](./todo.md) for the roadmap. The engine's design is in [`spec.txt`](./spec.txt).

## Getting started

```sh
# Rust workspace
cargo build --workspace
cargo test  --workspace
cargo clippy --workspace --all-targets   # must pass with no warnings

# Frontend (the wasm engine is built automatically by predev/prebuild)
cd frontend
npm install
npm run lint && npm run typecheck && npm test
npm run dev      # http://localhost:5173
```

A `Dockerfile`, `docker-compose.yml`, and `.devcontainer/` are provided for a
one-command setup (see [`README.md`](./README.md#quick-start-full-collaborative-app)).

## Before opening a pull request

1. Run `cargo fmt --all` and ensure `cargo clippy --workspace --all-targets`
   passes clean (CI treats warnings as errors).
2. Add or update tests for any behavior change:
   - Rust: `#[test]` in the relevant crate, plus `wasm-pack test` for wasm glue.
   - Frontend: `vitest` (`npm test`) for logic; keep the keyboard-navigation and
     sync tests green.
3. If you change `CellValue`, `CellId`, or the `SerializableGrid` shape, bump the
   `format_version` in `tpt-lattice-io` and add a round-trip test.
4. Update `todo.md` to reflect the new state of the item you worked on, and add a
   short note if you deferred or blocked something.
5. Keep commits focused; describe *why*, not just *what*.

## Code style

- Rust: default `rustfmt` + `clippy` lints; prefer explicit error propagation.
- TypeScript: `eslint` (flat config) + Prettier; `tsc --noEmit` must pass.
- Comments: explain non-obvious invariants (overflow guards, CRDT ordering, the
  `1_000_000`-cell rescan we deliberately removed, etc.).

## Reporting issues

Include a minimal reproduction: the cell addresses/formulas involved, expected vs.
actual values, and whether it reproduces single-user or only under collaboration.
For sync bugs, note the number of peers and the order of edits.
