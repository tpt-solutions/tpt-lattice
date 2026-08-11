# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2026-08-11

### Added
- `parse` and `parse_expr` entry points returning a typed `Formula` / `Expr`.
- `ast` module with `Formula`, `Expr`, `Literal`, `CellRef`, `BinaryOp`, `UnaryOp`, `CastKind`,
  `MatchArm`, and `MatchPattern` node types.
- Lexer (tokenizer) and recursive-descent parser for the LES grammar.
- Support for LES-specific syntax: `RANGE()`, `MATCH` with `Ok`/`Err` pattern matching, and
  explicit casts (`NUMBER`, `TEXT`, `BOOL`).
- Property-based round-trip tests (`proptest`) plus unit tests for known-good and known-error
  formulas.
- Optional `serde` feature serializing the AST.
