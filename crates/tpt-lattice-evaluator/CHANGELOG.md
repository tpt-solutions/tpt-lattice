# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2026-08-11

### Added
- `Evaluator`: sparse in-memory grid implementing `GridState`, with `set_value`, `set_formula`,
  `recompute`, `get_value`, `get_formula`, `is_formula`, and `dag()` accessors.
- `DependencyGraph`: tracks dependencies and dependents, with Tarjan SCC-based cycle detection
  (`cycle_cells`) and topological ordering (`topo_order_excluding`).
- DAG-walk evaluator walking an LES `Expr` against a `GridState`.
- Dirty-cell invalidation and incremental re-evaluation (only dirty cells recompute).
- `MAX_RANGE_CELLS` cap guarding `RANGE(...)` expansion.
- Tests: formula chains, cascading updates, circular references, range sums, strict typing,
  `MATCH` error handling, division by zero, and dirty invalidation.
