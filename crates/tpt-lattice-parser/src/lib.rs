//! # tpt-lattice-parser
//!
//! Lexing and parsing of the **Lattice Expression Syntax (LES)** into a strongly
//! typed AST ([`ast::Expr`]). The parser is `no_std`-compatible and built on
//! [`nom`]; see `parser.rs` for the grammar.
//!
//! ```
//! use tpt_lattice_parser::parse;
//! let formula = parse("=SUM(RANGE(A1, A3)) * 2").unwrap();
//! assert!(matches!(formula.body, tpt_lattice_parser::ast::Expr::Binary { .. }));
//! ```

#![no_std]
#![cfg_attr(docsrs, feature(doc_cfg))]

extern crate alloc;

#[cfg(feature = "serde")]
extern crate serde;

pub mod ast;
mod parser;

pub use ast::{
    BinaryOp, CastKind, CellRef, Expr, Formula, Literal, MatchArm, MatchPattern, UnaryOp,
};
pub use parser::{parse, parse_expr};

#[cfg(test)]
mod proptests;
