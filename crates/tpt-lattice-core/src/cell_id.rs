//! Compact cell addressing.
//!
//! A [`CellId`] packs a `(column, row)` coordinate into a single `u64`:
//!
//! ```text
//! bit layout:  [ 20 bits column | 44 bits row ]
//!              ^ MSB (bits 63..44)  ^ bits 43..0
//! ```
//!
//! Empty cells consume no memory (the grid is stored as a sparse map keyed by
//! `CellId`), so a dense, fixed 2D array is never allocated.

use alloc::string::{String, ToString};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::MAX_COLUMN;

/// Error returned when an A1-style address cannot be parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ColumnParseError(pub alloc::string::String);

impl ColumnParseError {
    /// Construct an error describing an unparseable address.
    pub fn new(msg: impl Into<alloc::string::String>) -> Self {
        ColumnParseError(msg.into())
    }
}

impl core::fmt::Display for ColumnParseError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "invalid cell address: {}", self.0)
    }
}

/// A compact, copyable cell coordinate.
///
/// Construct with [`CellId::from_rc`] (0-indexed) or parse from A1-style text
/// with [`CellId::from_a1`] / [`CellId::try_from_a1`]. Convert back with
/// [`CellId::to_rc`] or [`CellId::to_a1`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct CellId(u64);

impl CellId {
    /// Number of bits used for the row component (least significant).
    const ROW_BITS: u32 = 44;
    /// Mask isolating the row component.
    const ROW_MASK: u64 = (1 << Self::ROW_BITS) - 1;

    /// Create a `CellId` from 0-indexed `(column, row)` coordinates.
    ///
    /// # Panics
    /// Panics if `column > MAX_COLUMN` or `row > MAX_ROW`.
    pub fn from_rc(column: u64, row: u64) -> Self {
        assert!(column <= MAX_COLUMN, "column index out of range");
        assert!(row <= crate::MAX_ROW, "row index out of range");
        CellId((column << Self::ROW_BITS) | (row & Self::ROW_MASK))
    }

    /// Create a `CellId` from raw packed bits. Useful for serialization.
    pub const fn from_bits(bits: u64) -> Self {
        CellId(bits)
    }

    /// Return the packed `u64` representation.
    pub const fn to_bits(self) -> u64 {
        self.0
    }

    /// Return `(column, row)` as 0-indexed values.
    pub fn to_rc(self) -> (u64, u64) {
        (self.0 >> Self::ROW_BITS, self.0 & Self::ROW_MASK)
    }

    /// Column index (0-indexed).
    pub fn column(self) -> u64 {
        self.0 >> Self::ROW_BITS
    }

    /// Row index (0-indexed).
    pub fn row(self) -> u64 {
        self.0 & Self::ROW_MASK
    }

    /// Parse an A1-style address such as `B7` or `AA100`.
    ///
    /// Returns [`ColumnParseError`] if the string is empty, contains no column
    /// letters, no row digits, or encodes an out-of-range coordinate.
    pub fn try_from_a1(s: &str) -> Result<Self, ColumnParseError> {
        let s = s.trim();
        if s.is_empty() {
            return Err(ColumnParseError::new("empty cell address"));
        }

        let bytes = s.as_bytes();
        let mut i = 0;
        // Leading column letters (bijective base-26: A=0, B=1, ... AA=26).
        // Use checked arithmetic so 14+ letter columns overflow `u64` and are
        // rejected as out-of-range rather than wrapping into a corrupted id.
        let mut column: u64 = 0;
        let mut overflowed = false;
        while i < bytes.len() {
            let b = bytes[i];
            if !b.is_ascii_uppercase() && !b.is_ascii_lowercase() {
                break;
            }
            let digit = if b <= b'Z' { b - b'A' } else { b - b'a' } as u64;
            match column
                .checked_mul(26)
                .and_then(|c| c.checked_add(digit + 1))
            {
                Some(v) => column = v,
                None => overflowed = true,
            }
            i += 1;
        }
        // Bijective base-26 has no zero digit, so values are 1-based; convert.
        if overflowed {
            column = u64::MAX; // guarantees > MAX_COLUMN below
        }
        column = column.saturating_sub(1);

        if i == 0 {
            return Err(ColumnParseError::new("address has no column letters"));
        }
        if i >= bytes.len() {
            return Err(ColumnParseError::new("address has no row digits"));
        }

        let row_str = &s[i..];
        let row: u64 = row_str
            .parse()
            .map_err(|_| ColumnParseError::new("row is not a valid number"))?;
        if row == 0 {
            return Err(ColumnParseError::new("row index is 1-based; 0 is invalid"));
        }
        let row = row - 1; // 1-based input -> 0-based storage.

        if column > MAX_COLUMN {
            return Err(ColumnParseError::new("column index out of range"));
        }
        if row > crate::MAX_ROW {
            return Err(ColumnParseError::new("row index out of range"));
        }

        Ok(CellId::from_rc(column, row))
    }

    /// Parse an A1-style address, panicking on failure. Prefer [`CellId::try_from_a1`].
    pub fn from_a1(s: &str) -> Self {
        Self::try_from_a1(s).unwrap_or_else(|e| panic!("invalid cell address {s:?}: {e}"))
    }

    /// Render this id as an A1-style address (`A1`, `B2`, `AA100`, ...).
    pub fn to_a1(self) -> String {
        let (col, row) = self.to_rc();
        let mut letters = String::new();
        let mut c = col + 1;
        while c > 0 {
            let rem = (c - 1) % 26;
            letters.insert(0, (b'A' + rem as u8) as char);
            c = (c - 1) / 26;
        }
        let mut out = letters;
        out.push_str(&(row + 1).to_string());
        out
    }
}

impl core::fmt::Display for CellId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.to_a1())
    }
}

impl core::str::FromStr for CellId {
    type Err = ColumnParseError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        CellId::try_from_a1(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rc_roundtrip() {
        for (col, row) in [(0u64, 0u64), (1, 5), (25, 100), (26, 0), (702, 12345)] {
            let id = CellId::from_rc(col, row);
            assert_eq!(id.to_rc(), (col, row));
        }
    }

    #[test]
    fn bits_roundtrip() {
        let id = CellId::from_rc(123, 456);
        assert_eq!(CellId::from_bits(id.to_bits()), id);
    }

    #[test]
    fn a1_roundtrip() {
        for addr in ["A1", "B2", "Z26", "AA1", "AB27", "AZ100", "BA1000", "ZZ999"] {
            let id = CellId::from_a1(addr);
            assert_eq!(id.to_a1(), addr, "a1 roundtrip failed for {addr}");
        }
    }

    #[test]
    fn a1_case_insensitive() {
        assert_eq!(CellId::try_from_a1("a1"), CellId::try_from_a1("A1"));
    }

    #[test]
    fn a1_whitespace_trimmed() {
        assert_eq!(CellId::try_from_a1("  C3  "), CellId::try_from_a1("C3"));
    }

    #[test]
    fn a1_column_overflow_is_rejected() {
        // 14+ letter columns overflow u64 (and wasm32 usize) — must error rather
        // than panic or wrap into a corrupted CellId.
        assert!(CellId::try_from_a1("AAAAAAAAAAAAAA1").is_err());
        assert!(CellId::try_from_a1("ZZZZZZZZZZZZZZ1").is_err());
    }

    #[test]
    fn a1_invalid() {
        assert!(CellId::try_from_a1("").is_err());
        assert!(CellId::try_from_a1("123").is_err());
        assert!(CellId::try_from_a1("AB").is_err());
        assert!(CellId::try_from_a1("A0").is_err());
    }

    #[test]
    fn column_extraction() {
        let id = CellId::from_a1("B3");
        assert_eq!(id.column(), 1);
        assert_eq!(id.row(), 2);
    }

    #[test]
    fn from_str_trait() {
        use core::str::FromStr;
        assert_eq!(CellId::from_str("D4").unwrap(), CellId::from_a1("D4"));
    }
}
