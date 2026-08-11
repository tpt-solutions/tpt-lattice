# tpt-lattice-core

Foundational, [`no_std`](https://doc.rust-lang.org/stable/embedded-book/intro/no_std.html)-compatible
types shared by every other crate in the [TPT Lattice](https://github.com/tpt-solutions/tpt-lattice)
workspace — a next-generation, real-time collaborative spreadsheet engine built from first principles.

This crate defines the data model that everything else builds on. It has **zero** runtime
dependencies (only an optional `serde`) and no heap allocator required beyond `alloc`, so it
compiles cleanly to bare metal and WebAssembly.

## Features

- **`CellId`** — a compact `u64` bitfield packing a `(column, row)` coordinate, with A1-style
  parsing (`from_a1` / `try_from_a1`) and rendering (`to_a1`).
- **`CellValue`** — a strictly-typed cell value: `Empty`, `Number(f64)`, `Text(String)`,
  `Boolean(bool)`, or `Error(LatticeError)`. No implicit coercion between variants.
- **`LatticeError`** — an exhaustive first-class error hierarchy. Errors are *values* that
  propagate explicitly through formulas.
- **`GridState`** — the minimal read/write trait the evaluator, CRDT, and I/O layers operate
  against, so storage is fully decoupled from calculation.
- Optional `serde` feature adds `Serialize` / `Deserialize` for all public types.

## Installation

```toml
[dependencies]
tpt-lattice-core = "0.1.0"
```

Enable serialization:

```toml
tpt-lattice-core = { version = "0.1.0", features = ["serde"] }
```

## Usage

### Addressing cells

```rust
use tpt_lattice_core::CellId;

let id = CellId::try_from_a1("B3").unwrap();
assert_eq!(id.column(), 1); // 0-indexed
assert_eq!(id.row(), 2);
assert_eq!(id.to_a1(), "B3");

// Round-trips through raw packed bits for compact storage.
let bits = id.to_bits();
assert_eq!(CellId::from_bits(bits), id);
```

### Strictly-typed values

```rust
use tpt_lattice_core::{CellValue, LatticeError};

let n = CellValue::Number(3.0);
assert_eq!(n.as_number(), Some(3.0));
assert!(n.as_text().is_none());

// Non-finite numbers are normalized to an error value.
assert_eq!(
    CellValue::Number(f64::NAN).sanitize(),
    CellValue::Error(LatticeError::NotANumber)
);
```

### Reading and writing a grid

```rust
use tpt_lattice_core::{CellId, CellValue, GridState};
use std::collections::BTreeMap;

struct MapGrid { cells: BTreeMap<u64, CellValue> }
impl GridState for MapGrid {
    fn get_cell(&self, id: CellId) -> CellValue {
        self.cells.get(&id.to_bits()).cloned().unwrap_or(CellValue::Empty)
    }
    fn set_cell(&mut self, id: CellId, value: CellValue) {
        if value.is_empty() {
            self.cells.remove(&id.to_bits());
        } else {
            self.cells.insert(id.to_bits(), value);
        }
    }
}
```

## `no_std` support

The crate is `#![no_std]` and depends only on `core` and `alloc`. Bring your own allocator
(`extern crate alloc;`) when consuming it in a `no_std` environment.

## License

Licensed under either of [MIT](../../LICENSE-MIT) or [Apache-2.0](../../LICENSE-APACHE) at your
option.
