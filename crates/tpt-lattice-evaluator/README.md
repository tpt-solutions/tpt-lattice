# tpt-lattice-evaluator

The calculation engine at the heart of the
[TPT Lattice](https://github.com/tpt-solutions/tpt-lattice) spreadsheet: it builds a cell
dependency DAG, detects circular references, and incrementally evaluates LES formulas.

## Key ideas

- **Dependency graph** — a sparse directed graph tracking *who I need* (`dependencies`) and
  *who needs me* (`dependents`) for every `CellId`.
- **Cycle detection** — Tarjan's Strongly Connected Components identify circular references;
  affected cells evaluate to `LatticeError::CircularReference` while the rest of the grid
  computes normally.
- **Incremental evaluation** — editing a cell marks it and its transitive dependents *dirty*;
  `evaluate()` recomputes only dirty cells in topological order.
- **Strict typing** — values never coerce. `="5" + 5` is a `TypeError`; use `NUMBER("5") + 5`.
  Errors are first-class `CellValue::Error` values that propagate explicitly.

## Installation

```toml
[dependencies]
tpt-lattice-evaluator = "0.1.0"
```

## Usage

```rust
use tpt_lattice_core::{CellId, CellValue, GridState};
use tpt_lattice_evaluator::Evaluator;

let mut engine = Evaluator::new();
engine.set_value(CellId::from_a1("A1"), CellValue::Number(21.0));
engine.set_formula(CellId::from_a1("B1"), "=A1 * 2").unwrap();
engine.evaluate().unwrap();
assert_eq!(engine.get_value(CellId::from_a1("B1")), CellValue::Number(42.0));

// Cascading updates only recompute what changed.
engine.set_value(CellId::from_a1("A1"), CellValue::Number(10.0));
engine.evaluate().unwrap();
assert_eq!(engine.get_value(CellId::from_a1("B1")), CellValue::Number(20.0));
```

### Ranges and error handling

```rust
use tpt_lattice_core::{CellId, CellValue, LatticeError};
use tpt_lattice_evaluator::Evaluator;

let mut e = Evaluator::new();
for (c, v) in [("A1", 1.0), ("A2", 2.0), ("A3", 3.0)] {
    e.set_value(CellId::from_a1(c), CellValue::Number(v));
}
e.set_formula(CellId::from_a1("B1"), "=SUM(RANGE(A1, A3))").unwrap();
e.evaluate().unwrap();
assert_eq!(e.get_value(CellId::from_a1("B1")), CellValue::Number(6.0));

// Division by zero is a value, not a panic.
e.set_formula(CellId::from_a1("C1"), "=1 / 0").unwrap();
e.evaluate().unwrap();
assert_eq!(e.get_value(CellId::from_a1("C1")), CellValue::Error(LatticeError::DivByZero));
```

### Circular references

```rust
use tpt_lattice_core::{CellId, CellValue};
use tpt_lattice_evaluator::Evaluator;

let mut e = Evaluator::new();
e.set_formula(CellId::from_a1("A1"), "=B1 + 1").unwrap();
e.set_formula(CellId::from_a1("B1"), "=A1 + 1").unwrap();
e.evaluate().unwrap();
assert!(e.get_value(CellId::from_a1("A1")).is_error());
```

`Evaluator` implements `GridState`, so higher layers (CRDT, I/O) can read and write through it
transparently. Inspect the graph via `engine.dag()`.

## License

Licensed under either of [MIT](../../LICENSE-MIT) or [Apache-2.0](../../LICENSE-APACHE) at your
option.
