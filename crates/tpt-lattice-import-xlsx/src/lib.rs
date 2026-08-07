//! # tpt-lattice-import-xlsx
//!
//! Opt-in, feature-gated translation of legacy `.xlsx` workbooks into TPT
//! Lattice primitives using [`calamine`]. Unsupported Excel formulas become
//! explicit [`LatticeError::UnsupportedFormula`] values rather than silently
//! breaking downstream math.

use std::collections::BTreeMap;
use std::io::Cursor;

use calamine::{Data, Reader, Xlsx};
use tpt_lattice_core::{CellId, CellValue, LatticeError};

/// Errors that can occur while importing a workbook.
#[derive(Debug)]
pub enum ImportError {
    /// A calamine I/O or parsing failure.
    Calamine(String),
    /// The requested sheet was not found.
    SheetNotFound(String),
}

impl std::fmt::Display for ImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImportError::Calamine(e) => write!(f, "xlsx error: {e}"),
            ImportError::SheetNotFound(s) => write!(f, "sheet not found: {s}"),
        }
    }
}

impl std::error::Error for ImportError {}

impl From<calamine::Error> for ImportError {
    fn from(e: calamine::Error) -> Self {
        ImportError::Calamine(e.to_string())
    }
}

/// A single imported sheet: a sparse map of cell coordinates to values.
#[derive(Debug, Clone, Default)]
pub struct ImportedSheet {
    /// Cell values keyed by `CellId`.
    pub cells: BTreeMap<CellId, CellValue>,
    /// Named ranges discovered in the workbook (`name -> first cell`).
    pub named_ranges: BTreeMap<String, CellId>,
}

/// Import the **first** worksheet of an `.xlsx` workbook.
pub fn import_first_sheet(bytes: &[u8]) -> Result<ImportedSheet, ImportError> {
    let workbook = Xlsx::new(Cursor::new(bytes.to_vec()))
        .map_err(|e| ImportError::Calamine(e.to_string()))?;
    let name = workbook
        .sheet_names()
        .first()
        .cloned()
        .ok_or_else(|| ImportError::SheetNotFound("(no sheets)".into()))?;
    import_sheet(bytes, &name)
}

/// Import a specific worksheet by name.
pub fn import_sheet(bytes: &[u8], sheet_name: &str) -> Result<ImportedSheet, ImportError> {
    let mut workbook = Xlsx::new(Cursor::new(bytes.to_vec()))
        .map_err(|e| ImportError::Calamine(e.to_string()))?;
    let range = workbook
        .worksheet_range(sheet_name)
        .map_err(|e| ImportError::Calamine(e.to_string()))?;
    let formulas = workbook.worksheet_formula(sheet_name).ok();

    let mut sheet = ImportedSheet::default();

    for (i, row) in range.rows().enumerate() {
        for (j, cell) in row.iter().enumerate() {
            let id = CellId::from_rc(j as u64, i as u64);
            let value = map_cell(cell);

            // If Excel stored a formula for this cell, LES cannot represent it
            // faithfully yet, so surface it as an explicit error value.
            let has_formula = formulas
                .as_ref()
                .and_then(|f| f.get((i, j)))
                .map(|d| !d.is_empty())
                .unwrap_or(false);

            let value = if has_formula {
                CellValue::Error(LatticeError::unsupported(cell_to_text(cell)))
            } else {
                value
            };

            if !value.is_empty() {
                sheet.cells.insert(id, value);
            }
        }
    }

    // Best-effort named-range capture: map each name to its first referenced cell.
    for (name, src) in workbook.defined_names() {
        if let Some((sheet_part, coord)) = src.split_once('!') {
            let _ = sheet_part;
            if let Ok(id) = parse_coord(coord) {
                sheet.named_ranges.insert(name.clone(), id);
            }
        }
    }

    Ok(sheet)
}

fn map_cell(data: &Data) -> CellValue {
    match data {
        Data::Int(i) => CellValue::Number(*i as f64),
        Data::Float(f) => CellValue::Number(*f),
        Data::String(s) => CellValue::Text(s.clone()),
        Data::Bool(b) => CellValue::Boolean(*b),
        Data::DateTime(dt) => CellValue::Text(dt.to_string()),
        Data::DateTimeIso(s) => CellValue::Text(s.clone()),
        Data::DurationIso(s) => CellValue::Text(s.clone()),
        Data::Error(e) => CellValue::Error(LatticeError::ref_error(e.to_string())),
        Data::Empty => CellValue::Empty,
    }
}

fn cell_to_text(data: &Data) -> String {
    match data {
        Data::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Parse an Excel `$A$1`-style coordinate into a [`CellId`].
fn parse_coord(s: &str) -> Result<CellId, LatticeError> {
    let cleaned: String = s.chars().filter(|c| !c.is_ascii_punctuation()).collect();
    CellId::try_from_a1(&cleaned).map_err(|e| LatticeError::ref_error(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_panic_on_empty() {
        // An empty byte slice should fail gracefully, not panic.
        let result = import_first_sheet(b"not a real xlsx");
        assert!(result.is_err());
    }
}
