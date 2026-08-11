# tpt-lattice-parser

A [`no_std`](https://doc.rust-lang.org/stable/embedded-book/intro/no_std.html)-compatible
lexer and parser for the **Lattice Expression Syntax (LES)**, the formula language of the
[TPT Lattice](https://github.com/tpt-solutions/tpt-lattice) spreadsheet engine.

LES abandons spreadsheet legacy quirks in favor of strict typing and explicit, first-class
errors. This crate turns a formula string into a strongly-typed `ast::Expr` tree that the
evaluator walks.

## Features

- Hand-written **lexer** and **recursive-descent parser** built on [`nom`](https://docs.rs/nom).
- Full **LES AST** (`ast` module): literals, cell refs, named identifiers, unary/binary
  operations, function calls, explicit casts, and `Ok`/`Err` `MATCH` pattern matching.
- LES-specific syntax: `RANGE(A1, B10)` for explicit ranges, `MATCH(x, Ok(v) => ..., Err(e) => ...)`,
  and `NUMBER(...)` / `TEXT(...)` / `BOOL(...)` casts.
- Property-based tests (`proptest`) proving parser round-trips.
- Optional `serde` feature for AST serialization.

## Installation

```toml
[dependencies]
tpt-lattice-parser = "0.1.0"
```

## Usage

```rust
use tpt_lattice_parser::{parse, ast::Expr};

let formula = parse("=SUM(RANGE(A1, A3)) * 2").unwrap();
assert!(matches!(formula.body, Expr::Binary { .. }));

// Inspect the AST.
let formula = parse("=MATCH(A1, Ok(v) => v * 2, Err(e) => 0)").unwrap();
println!("{:#?}", formula.body);
```

You can also parse a bare expression without the leading `=`:

```rust
use tpt_lattice_parser::parse_expr;

let expr = parse_expr("1 + 2 * 3").unwrap();
println!("{expr:?}");
```

## Error handling

A malformed formula yields a `LatticeError::ParseError` describing the failure, never a panic:

```rust
use tpt_lattice_core::LatticeError;
use tpt_lattice_parser::parse;

let err = parse("=1 +").unwrap_err();
assert!(matches!(err, LatticeError::ParseError(_)));
```

## `no_std` support

The crate is `#![no_std]` and depends only on `core`, `alloc`, and `nom`.

## License

Licensed under either of [MIT](../../LICENSE-MIT) or [Apache-2.0](../../LICENSE-APACHE) at your
option.
