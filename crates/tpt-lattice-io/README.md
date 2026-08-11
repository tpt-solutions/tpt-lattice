# tpt-lattice-io

MessagePack and compact JSON serialization for the native
[TPT Lattice](https://github.com/tpt-solutions/tpt-lattice) grid state.

Both formats are fully round-trippable: serialize → deserialize → assert equality. The
snapshot captures *materialized* cell values (the computed truth) for transport and
persistence; formulas live in the evaluator, not in the serialized snapshot.

## Features

- **`SerializableGrid`** — a sparse, deterministic snapshot of `(CellId, CellValue)` pairs.
- **MessagePack** via `to_msgpack` / `from_msgpack` (`rmp-serde`).
- **Compact JSON** via `to_json` / `from_json`.
- `from_grid` builds a snapshot from anything implementing `GridState`.
- `SerializableGrid` itself implements `GridState`, so it slots into the engine ecosystem.

## Installation

```toml
[dependencies]
tpt-lattice-io = "0.1.0"
```

## Usage

```rust
use tpt_lattice_core::{CellId, CellValue};
use tpt_lattice_io::{SerializableGrid, to_msgpack, from_msgpack, to_json, from_json};

let mut g = SerializableGrid::new();
g.set(CellId::from_a1("A1"), CellValue::Number(3.0));
g.set(CellId::from_a1("B2"), CellValue::Text("hi".into()));

// MessagePack round-trip.
let bytes = to_msgpack(&g).unwrap();
assert_eq!(from_msgpack(&bytes).unwrap(), g);

// JSON round-trip.
let s = to_json(&g).unwrap();
assert_eq!(from_json(&s).unwrap(), g);

// Empty cells are dropped automatically.
g.set(CellId::from_a1("A1"), CellValue::Empty);
assert!(g.is_empty());
```

## License

Licensed under either of [MIT](../../LICENSE-MIT) or [Apache-2.0](../../LICENSE-APACHE) at your
option.
