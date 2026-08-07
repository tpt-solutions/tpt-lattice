//! The strictly-typed value that may be stored in a cell.

use alloc::string::String;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::error::LatticeError;

/// A cell's value. Empty cells are represented explicitly as [`CellValue::Empty`]
/// rather than by absence, so that errors and values are always distinguishable.
///
/// LES is strictly typed: there is no implicit coercion between variants.
/// Attempting to add a [`CellValue::Text`] to a [`CellValue::Number`] produces a
/// [`LatticeError::TypeError`] rather than a silent `10`.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum CellValue {
    /// An empty / unset cell. Consumes no storage in a sparse grid.
    Empty,
    /// An IEEE-754 finite number. Non-finite values (`NaN`, `inf`) are rejected
    /// by the evaluator and stored as errors instead.
    Number(f64),
    /// A UTF-8 text string.
    Text(String),
    /// A boolean.
    Boolean(bool),
    /// A first-class, explicitly represented error.
    Error(LatticeError),
}

impl CellValue {
    /// Whether this value is the canonical empty cell.
    pub fn is_empty(&self) -> bool {
        matches!(self, CellValue::Empty)
    }

    /// Whether this value is an [`CellValue::Error`].
    pub fn is_error(&self) -> bool {
        matches!(self, CellValue::Error(_))
    }

    /// If this value is a number, return it; otherwise `None`.
    pub fn as_number(&self) -> Option<f64> {
        match self {
            CellValue::Number(n) => Some(*n),
            _ => None,
        }
    }

    /// If this value is text, return a string slice; otherwise `None`.
    pub fn as_text(&self) -> Option<&str> {
        match self {
            CellValue::Text(s) => Some(s),
            _ => None,
        }
    }

    /// If this value is a boolean, return it; otherwise `None`.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            CellValue::Boolean(b) => Some(*b),
            _ => None,
        }
    }

    /// If this value is an error, return a reference; otherwise `None`.
    pub fn as_error(&self) -> Option<&LatticeError> {
        match self {
            CellValue::Error(e) => Some(e),
            _ => None,
        }
    }

    /// Normalize `NaN`/`±inf` numbers into a [`LatticeError::NotANumber`] error
    /// value. Finite numbers and every other variant pass through unchanged.
    pub fn sanitize(self) -> Self {
        if let CellValue::Number(n) = self {
            if !n.is_finite() {
                return CellValue::Error(LatticeError::NotANumber);
            }
        }
        self
    }
}

impl Default for CellValue {
    fn default() -> Self {
        CellValue::Empty
    }
}

impl From<f64> for CellValue {
    fn from(n: f64) -> Self {
        CellValue::Number(n)
    }
}

impl From<i64> for CellValue {
    fn from(n: i64) -> Self {
        CellValue::Number(n as f64)
    }
}

impl From<bool> for CellValue {
    fn from(b: bool) -> Self {
        CellValue::Boolean(b)
    }
}

impl From<&str> for CellValue {
    fn from(s: &str) -> Self {
        CellValue::Text(alloc::string::String::from(s))
    }
}

impl From<LatticeError> for CellValue {
    fn from(e: LatticeError) -> Self {
        CellValue::Error(e)
    }
}

impl core::fmt::Display for CellValue {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            CellValue::Empty => Ok(()),
            CellValue::Number(n) => write!(f, "{n}"),
            CellValue::Text(s) => write!(f, "{s}"),
            CellValue::Boolean(b) => write!(f, "{b}"),
            CellValue::Error(e) => write!(f, "#{e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::LatticeError;

    #[test]
    fn downcasts() {
        assert_eq!(CellValue::Number(3.0).as_number(), Some(3.0));
        assert_eq!(CellValue::Text("hi".into()).as_text(), Some("hi"));
        assert_eq!(CellValue::Boolean(true).as_bool(), Some(true));
        assert!(CellValue::Number(1.0).as_text().is_none());
    }

    #[test]
    fn sanitize_nonfinite() {
        assert_eq!(CellValue::Number(f64::NAN).sanitize(), CellValue::Error(LatticeError::NotANumber));
        assert_eq!(CellValue::Number(f64::INFINITY).sanitize(), CellValue::Error(LatticeError::NotANumber));
        assert_eq!(CellValue::Number(1.0).sanitize(), CellValue::Number(1.0));
    }

    #[test]
    fn from_conversions() {
        assert_eq!(CellValue::from(2.0), CellValue::Number(2.0));
        assert_eq!(CellValue::from(true), CellValue::Boolean(true));
        assert_eq!(CellValue::from("x"), CellValue::Text("x".into()));
        assert!(CellValue::from(LatticeError::DivByZero).is_error());
    }
}
