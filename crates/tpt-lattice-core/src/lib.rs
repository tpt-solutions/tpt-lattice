//! # tpt-lattice-core
//!
//! Foundational, `no_std`-compatible types for the TPT Lattice spreadsheet engine.
//!
//! This crate defines the data model shared by every other crate in the workspace:
//!
//! - [`CellId`] — a compact `u64` bitfield encoding a `(column, row)` pair.
//! - [`CellValue`] — the strictly-typed value stored in a cell.
//! - [`LatticeError`] — a first-class error hierarchy.
//! - [`GridState`] — the read/write interface the evaluator operates against.
//!
//! When the `serde` feature is enabled, all public types gain `Serialize` /
//! `Deserialize` implementations.

#![no_std]
#![cfg_attr(docsrs, feature(doc_cfg))]

extern crate alloc;

#[cfg(feature = "serde")]
extern crate serde;

mod cell_id;
mod cell_value;
mod error;
mod grid;

pub use cell_id::{CellId, ColumnParseError};
pub use cell_value::{
    format_serial_date, serial_from_ymd, ymd_from_serial, CellValue, EXCEL_EPOCH_OFFSET,
};
pub use error::LatticeError;
pub use grid::GridState;

/// The maximum representable column index (20-bit field).
pub const MAX_COLUMN: u64 = (1 << 20) - 1;
/// The maximum representable row index (44-bit field).
pub const MAX_ROW: u64 = (1 << 44) - 1;
