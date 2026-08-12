//! The strictly-typed value that may be stored in a cell.

use alloc::string::String;
use alloc::vec::Vec;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::error::LatticeError;

/// A cell's value. Empty cells are represented explicitly as [`CellValue::Empty`]
/// rather than by absence, so that errors and values are always distinguishable.
///
/// LES is strictly typed: there is no implicit coercion between variants.
/// Attempting to add a [`CellValue::Text`] to a [`CellValue::Number`] produces a
/// [`LatticeError::TypeError`] rather than a silent `10`.
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum CellValue {
    /// An empty / unset cell. Consumes no storage in a sparse grid.
    #[default]
    Empty,
    /// An IEEE-754 finite number. Non-finite values (`NaN`, `inf`) are rejected
    /// by the evaluator and stored as errors instead.
    Number(f64),
    /// A UTF-8 text string.
    Text(String),
    /// A boolean.
    Boolean(bool),
    /// A calendar date (and optionally time), stored as an Excel-style serial
    /// number: the integer part is the number of days since the 1899-12-30 epoch
    /// and the fractional part is the time of day. Rendered as `YYYY-MM-DD` (or
    /// `YYYY-MM-DD HH:MM:SS` when a time is present).
    Date(f64),
    /// An ordered list of values (produced by e.g. `SPLIT`). There is no implicit
    /// coercion to or from scalars.
    List(Vec<CellValue>),
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
        match self {
            CellValue::Number(n) if !n.is_finite() => {
                CellValue::Error(LatticeError::NotANumber)
            }
            CellValue::Date(s) if !s.is_finite() => CellValue::Error(LatticeError::NotANumber),
            other => other,
        }
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
            CellValue::Date(s) => write!(f, "{}", format_serial_date(*s)),
            CellValue::List(items) => {
                let parts: Vec<String> = items.iter().map(|v| v.to_string()).collect();
                write!(f, "{}", parts.join(", "))
            }
            CellValue::Error(e) => write!(f, "#{e}"),
        }
    }
}

/// Excel epoch offset: `serial 0` corresponds to 1899-12-30, and the Unix epoch
/// (1970-01-01) is serial `25569`. Used to translate between civil dates and the
/// serial numbers stored in [`CellValue::Date`].
const EXCEL_EPOCH_OFFSET: i64 = 25569;

/// Convert a day count since the Unix epoch into a `(year, month, day)` civil date
/// (Howard Hinnant's `civil_from_days` algorithm).
fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365; // [0,399]
    let y = (yoe + era * 400) as i32;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0,365]
    let mp = (5 * doy + 2) / 153; // [0,11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1,31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1,12]
    let y = if m <= 2 { y + 1 } else { y };
    (y, m as u32, d as u32)
}

/// Inverse of [`civil_from_days`]: civil date -> days since the Unix epoch.
fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y } as i64;
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as i64; // [0,399]
    let m = m as i64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d as i64 - 1; // [0,365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0,146096]
    era * 146_097 + doe - 719_468
}

/// Render an Excel serial number as `YYYY-MM-DD` or `YYYY-MM-DD HH:MM:SS`.
pub fn format_serial_date(serial: f64) -> String {
    if !serial.is_finite() {
        return "#NUM!".to_string();
    }
    let days = serial.floor() as i64;
    let unix_days = days - EXCEL_EPOCH_OFFSET;
    let (y, m, d) = civil_from_days(unix_days);
    let frac = serial - serial.floor();
    if frac.abs() < 1e-9 {
        return format!("{y:04}-{m:02}-{d:02}");
    }
    let secs = (frac * 86_400.0).round() as i64;
    let hh = (secs / 3600) % 24;
    let mm = (secs / 60) % 60;
    let ss = secs % 60;
    format!("{y:04}-{m:02}-{d:02} {hh:02}:{mm:02}:{ss:02}")
}

/// Build an Excel serial number from a `(year, month, day)` civil date.
pub fn serial_from_ymd(y: i32, m: u32, d: u32) -> f64 {
    (days_from_civil(y, m, d) + EXCEL_EPOCH_OFFSET) as f64
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
        assert_eq!(
            CellValue::Number(f64::NAN).sanitize(),
            CellValue::Error(LatticeError::NotANumber)
        );
        assert_eq!(
            CellValue::Number(f64::INFINITY).sanitize(),
            CellValue::Error(LatticeError::NotANumber)
        );
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
