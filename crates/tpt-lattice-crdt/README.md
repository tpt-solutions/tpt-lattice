# tpt-lattice-crdt

An operation-based **CRDT** (Conflict-free Replicated Data Type) for offline-first, real-time
collaborative grid mutations in the
[TPT Lattice](https://github.com/tpt-solutions/tpt-lattice) spreadsheet engine.

Every mutation is expressed as an `Op` carrying a causal `VectorClock` and the originating
actor id. Applying ops uses a deterministic **last-writer-wins** rule keyed on
`(clock_total, actor)`, which makes merge **commutative** and **associative** — any two peers
that have seen the same set of ops converge to identical state regardless of arrival order.

Rows and columns carry immutable [`ulid::Ulid`](https://docs.rs/ulid) identifiers (never integer
indices), so concurrent edits to the same region cannot corrupt each other.

## Features

- **`Op`** — `SetCell`, `DeleteCell`, `InsertRow`, `InsertColumn`, `DeleteRow`, `DeleteColumn`.
- **`VectorClock`** — a version vector for causal ordering, divergence detection (`happens_before`,
  `concurrent`), and ticking.
- **`CrdtStore`** — the materialized, conflict-free grid. Author ops locally (`set_cell`,
  `delete_cell`, `insert_row`, ...) or merge a peer's op log (`merge_ops`).
- Deterministic LWW precedence for conflict resolution.
- Convergence guaranteed by order-independent op application.

## Installation

```toml
[dependencies]
tpt-lattice-crdt = "0.1.0"
```

## Usage

### Authoring and merging

```rust
use tpt_lattice_core::{CellId, CellValue};
use tpt_lattice_crdt::CrdtStore;

let mut peer_a = CrdtStore::new(1);
let mut peer_b = CrdtStore::new(2);

// Each peer edits offline...
let ops_a = vec![
    peer_a.set_cell(CellId::from_a1("A1"), CellValue::Number(10.0)),
    peer_a.set_cell(CellId::from_a1("A2"), CellValue::Number(20.0)),
];
let ops_b = vec![
    peer_b.set_cell(CellId::from_a1("B1"), CellValue::Text("x".into())),
    peer_b.set_cell(CellId::from_a1("A1"), CellValue::Number(999.0)), // concurrent edit on A1
];

// Exchange logs — order independent.
peer_a.merge_ops(ops_b.iter().cloned());
peer_b.merge_ops(ops_a.iter().cloned());

// Both converge to the identical state.
assert_eq!(peer_a.get_cell(CellId::from_a1("A1")), peer_b.get_cell(CellId::from_a1("A1")));
assert_eq!(peer_a.get_cell(CellId::from_a1("A1")), CellValue::Number(999.0));
```

### Structural edits

```rust
use tpt_lattice_crdt::CrdtStore;

let mut s = CrdtStore::new(1);
let (r1, _) = s.insert_row(None);
let (r2, _) = s.insert_row(Some(r1));
assert_eq!(s.row_count(), 2);
```

## Integration

`CrdtStore` exposes materialized `CellValue`s via `get_cell`, and the WASM layer
(`tpt-lattice-wasm`) re-materializes the evaluator from the CRDT after merging remote ops,
so op application flows into DAG invalidation and re-evaluation.

## License

Licensed under either of [MIT](../../LICENSE-MIT) or [Apache-2.0](../../LICENSE-APACHE) at your
option.
