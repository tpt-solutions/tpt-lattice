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

mod xlsx;

pub use xlsx::{CellStyle, HorizontalAlign, MergedRegion, VerticalAlign};

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
    /// Merged-cell regions, each described by its bounding `CellId`s.
    pub merged_cells: Vec<MergedRegion>,
    /// Per-cell styling carried over from the workbook, keyed by `CellId`.
    pub styles: BTreeMap<CellId, CellStyle>,
}

/// Import the **first** worksheet of an `.xlsx` workbook.
pub fn import_first_sheet(bytes: &[u8]) -> Result<ImportedSheet, ImportError> {
    let workbook =
        Xlsx::new(Cursor::new(bytes.to_vec())).map_err(|e| ImportError::Calamine(e.to_string()))?;
    let name = workbook
        .sheet_names()
        .first()
        .cloned()
        .ok_or_else(|| ImportError::SheetNotFound("(no sheets)".into()))?;
    import_sheet(bytes, &name)
}

/// Import a specific worksheet by name.
pub fn import_sheet(bytes: &[u8], sheet_name: &str) -> Result<ImportedSheet, ImportError> {
    let mut workbook =
        Xlsx::new(Cursor::new(bytes.to_vec())).map_err(|e| ImportError::Calamine(e.to_string()))?;
    let range = workbook
        .worksheet_range(sheet_name)
        .map_err(|e| ImportError::Calamine(e.to_string()))?;

    // The formula range is indexed *relative to its own top-left*, which is not
    // necessarily A1. Build an absolute-coordinate map so lookups line up with
    // the data range below.
    let mut formulas_by_cell: BTreeMap<(usize, usize), String> = BTreeMap::new();
    if let Ok(fr) = workbook.worksheet_formula(sheet_name) {
        let (fsr, fsc) = fr.start().unwrap_or((0, 0));
        for (r, frow) in fr.rows().enumerate() {
            for (c, formula) in frow.iter().enumerate() {
                if !formula.is_empty() {
                    formulas_by_cell.insert((fsr as usize + r, fsc as usize + c), formula.clone());
                }
            }
        }
    }

    let (dsr, dsc) = range.start().unwrap_or((0, 0));
    let mut sheet = ImportedSheet::default();

    for (i, row) in range.rows().enumerate() {
        for (j, cell) in row.iter().enumerate() {
            // Absolute (row, col) in the sheet for this data cell.
            let abs = (dsr as usize + i, dsc as usize + j);
            let id = CellId::from_rc(abs.1 as u64, abs.0 as u64);
            let value = map_cell(cell);

            // If Excel stored a formula for this cell, LES cannot represent it
            // faithfully yet, so surface it as an explicit error value.
            let value = match formulas_by_cell.get(&abs) {
                Some(formula) => CellValue::Error(LatticeError::unsupported(format!("={formula}"))),
                None => value,
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

    // Carry merged-cell regions and per-cell styles through from the raw XML.
    xlsx::attach_styles_and_merges(bytes, sheet_name, &mut sheet);

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
