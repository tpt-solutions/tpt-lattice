//! The first-class error hierarchy.
//!
//! Errors are values. A [`CellValue::Error`] stores a [`LatticeError`] and
//! propagates explicitly through formulas (there is no implicit coercion and no
//! silent `#N/A` string scrambling downstream math).

use alloc::string::String;
use core::fmt;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Every error the Lattice engine can surface.
///
/// Variants are exhaustive so downstream crates (the evaluator, the importer)
/// can pattern-match with confidence. New error kinds are added here only after
/// a semver-minor review.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum LatticeError {
    /// A token could not be lexed or the formula could not be parsed.
    ParseError(String),
    /// A value of the wrong type was used where another was expected.
    TypeError {
        /// The type the operation required.
        expected: String,
        /// The type that was actually supplied.
        got: String,
    },
    /// Division (or modulo) by zero.
    DivByZero,
    /// A referenced name (variable or function) is not defined.
    NameError(String),
    /// A cell reference is out of bounds or refers to a deleted item.
    RefError(String),
    /// A value was used that is not a finite number (`NaN` / `inf`).
    NotANumber,
    /// A circular dependency was detected while evaluating.
    CircularReference(String),
    /// A value is not available / not found (the `#N/A` family of errors).
    /// Returned by lookup functions (e.g. `VLOOKUP`) when no match exists, and
    /// recognized by the `ISNA` predicate.
    NA,
    /// A formula references a function not supported by LES.
    UnsupportedFormula(String),
    /// A function received the wrong number of arguments.
    ArgumentError(String),
    /// Catch-all for errors originating outside the engine (I/O, CRDT, ...).
    Internal(String),
}

impl LatticeError {
    /// Convenience constructor for a [`LatticeError::TypeError`].
    pub fn type_error(expected: impl Into<String>, got: impl Into<String>) -> Self {
        LatticeError::TypeError {
            expected: expected.into(),
            got: got.into(),
        }
    }

    /// Convenience constructor for a [`LatticeError::NameError`].
    pub fn name_error(name: impl Into<String>) -> Self {
        LatticeError::NameError(name.into())
    }

    /// Convenience constructor for a [`LatticeError::RefError`].
    pub fn ref_error(detail: impl Into<String>) -> Self {
        LatticeError::RefError(detail.into())
    }

    /// Convenience constructor for a [`LatticeError::ArgumentError`].
    pub fn argument_error(detail: impl Into<String>) -> Self {
        LatticeError::ArgumentError(detail.into())
    }

    /// Convenience constructor for a [`LatticeError::NA`].
    pub fn na() -> Self {
        LatticeError::NA
    }

    /// Convenience constructor for a [`LatticeError::UnsupportedFormula`].
    pub fn unsupported(detail: impl Into<String>) -> Self {
        LatticeError::UnsupportedFormula(detail.into())
    }

    /// Convenience constructor for a [`LatticeError::Internal`].
    pub fn internal(detail: impl Into<String>) -> Self {
        LatticeError::Internal(detail.into())
    }
}

impl fmt::Display for LatticeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Render with familiar Excel-style error codes so that a failed cell
        // reads `#DIV/0!` rather than a prose message. Variant *names* are
        // unchanged, so serialized history and downstream matches keep working.
        match self {
            LatticeError::ParseError(_) => write!(f, "#ERROR!"),
            LatticeError::TypeError { .. } => write!(f, "#VALUE!"),
            LatticeError::DivByZero => write!(f, "#DIV/0!"),
            LatticeError::NameError(_) => write!(f, "#NAME?"),
            LatticeError::RefError(_) => write!(f, "#REF!"),
            LatticeError::NotANumber => write!(f, "#NUM!"),
            LatticeError::NA => write!(f, "#N/A"),
            LatticeError::CircularReference(_) => write!(f, "#CIRC!"),
            LatticeError::UnsupportedFormula(_) => write!(f, "#ERROR!"),
            LatticeError::ArgumentError(_) => write!(f, "#ERROR!"),
            LatticeError::Internal(_) => write!(f, "#ERROR!"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;

    #[test]
    fn display_formats() {
        assert_eq!(LatticeError::DivByZero.to_string(), "#DIV/0!");
        assert_eq!(
            LatticeError::type_error("Number", "Text").to_string(),
            "#VALUE!"
        );
        assert_eq!(LatticeError::name_error("FOO").to_string(), "#NAME?");
        assert_eq!(LatticeError::na().to_string(), "#N/A");
    }

    #[test]
    fn eq_works() {
        assert_eq!(LatticeError::DivByZero, LatticeError::DivByZero);
        assert_ne!(LatticeError::DivByZero, LatticeError::NotANumber);
    }
}
