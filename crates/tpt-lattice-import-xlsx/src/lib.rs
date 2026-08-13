//! # tpt-lattice-import-xlsx
//!
//! Opt-in, feature-gated translation of legacy `.xlsx` workbooks into TPT
//! Lattice primitives using [`calamine`]. Unsupported Excel formulas become
//! explicit [`LatticeError::UnsupportedFormula`] values rather than silently
//! breaking downstream math.

use std::collections::BTreeMap;
use std::io::Cursor;

use calamine::{Data, Reader, Xlsx};
use tpt_lattice_core::{serial_from_ymd, CellId, CellValue, LatticeError};

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
    /// Source formulas translated from Excel syntax into LES, keyed by `CellId`.
    /// Present only for cells whose formula LES can represent; unsupported
    /// formulas instead surface as `CellValue::Error(UnsupportedFormula)` in
    /// [`cells`].
    pub formulas: BTreeMap<CellId, String>,
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

/// Import **every** worksheet of a workbook, returning them in workbook order
/// alongside their names. Useful for round-tripping multi-sheet books or importing
/// a whole document at once.
pub fn import_all_sheets(bytes: &[u8]) -> Result<Vec<(String, ImportedSheet)>, ImportError> {
    let workbook =
        Xlsx::new(Cursor::new(bytes.to_vec())).map_err(|e| ImportError::Calamine(e.to_string()))?;
    let names = workbook.sheet_names().to_vec();
    let mut out = Vec::with_capacity(names.len());
    for name in names {
        out.push((name.clone(), import_sheet(bytes, &name)?));
    }
    Ok(out)
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
            let cached = map_cell(cell);

            // If Excel stored a formula for this cell, translate it into LES and
            // carry it in `formulas`. Cells whose formula LES cannot represent are
            // surfaced as an explicit `UnsupportedFormula` error instead.
            let value = match formulas_by_cell.get(&abs) {
                Some(formula) => match translate_excel_to_les(formula) {
                    Some(les) => {
                        sheet.formulas.insert(id, les);
                        // Keep any cached computed value; otherwise leave the cell
                        // empty (the formula is recorded separately).
                        if cached.is_empty() {
                            continue;
                        }
                        cached
                    }
                    None => CellValue::Error(LatticeError::unsupported(format!("={formula}"))),
                },
                None => cached,
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

/// Parse an ISO-8601 date/time string (e.g. `2020-03-15` or
/// `2020-03-15T10:30:00`) into an Excel-style serial number. Returns `None` if
/// the string is not a recognizable date.
fn iso_to_date(s: &str) -> Option<f64> {
    let date_part = s.split(['T', ' ']).next()?;
    let mut parts = date_part.split('-');
    let y: i32 = parts.next()?.parse().ok()?;
    let m: u32 = parts.next()?.parse().ok()?;
    let d: u32 = parts.next()?.parse().ok()?;
    let mut serial = serial_from_ymd(y, m, d);
    // Optional time component after a 'T' or space separator.
    if let Some(idx) = s[date_part.len()..].find(|c| c == 'T' || c == ' ') {
        let time = &s[date_part.len() + idx + 1..];
        let mut tp = time.split(':');
        if let (Some(hs), Some(ms), Some(ss)) = (tp.next(), tp.next(), tp.next()) {
            let h: f64 = hs.parse().ok()?;
            let m2: f64 = ms.parse().ok()?;
            let s2: f64 = ss.parse().ok()?;
            serial += (h * 3600.0 + m2 * 60.0 + s2) / 86_400.0;
        }
    }
    Some(serial)
}

fn map_cell(data: &Data) -> CellValue {
    match data {
        Data::Int(i) => CellValue::Number(*i as f64),
        Data::Float(f) => CellValue::Number(*f),
        Data::String(s) => CellValue::Text(s.clone()),
        Data::Bool(b) => CellValue::Boolean(*b),
        Data::DateTime(dt) => CellValue::Date(dt.as_f64()),
        Data::DateTimeIso(s) => match iso_to_date(s) {
            Some(serial) => CellValue::Date(serial),
            None => CellValue::Text(s.clone()),
        },
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

/// Translate an Excel formula string into LES syntax. Returns `None` when the
/// formula uses constructs LES does not support (structured references, table
/// array constants, external links, etc.).
///
/// Supported rewrites today:
/// * strip the leading `=`
/// * Excel ranges `A1:B2` become `RANGE(A1,B2)` (LES's explicit range form)
///
/// The result is best-effort: LES and Excel share much of their expression syntax,
/// so many formulas pass through unchanged, but callers should still validate the
/// produced formula against the LES parser.
pub fn translate_excel_to_les(excel: &str) -> Option<String> {
    let body = excel.trim().strip_prefix('=').unwrap_or(excel).trim();
    if body.is_empty() {
        return None;
    }
    // Reject constructs LES cannot represent.
    for bad in ['@', '[', '!', '{', '}'] {
        if body.contains(bad) {
            return None;
        }
    }
    let rewritten = rewrite_ranges(body);
    Some(format!("={rewritten}"))
}

/// Rewrite Excel `A1:B2` ranges into LES's `RANGE(A1,B2)` form.
fn rewrite_ranges(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        if let Some((end, a)) = read_cellref(&chars, i) {
            let mut j = end;
            while j < chars.len() && chars[j].is_whitespace() {
                j += 1;
            }
            if j < chars.len() && chars[j] == ':' {
                let mut k = j + 1;
                while k < chars.len() && chars[k].is_whitespace() {
                    k += 1;
                }
                if let Some((end2, b)) = read_cellref(&chars, k) {
                    out.push_str(&format!("RANGE({a},{b})"));
                    i = end2;
                    continue;
                }
            }
            out.push_str(&a);
            i = end;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

/// Read a cell reference (`$`-prefixed or bare `A1`) starting at `i`. Returns the
/// index just past it and the canonical (uppercased, `$`-stripped) A1 string.
fn read_cellref(chars: &[char], i: usize) -> Option<(usize, String)> {
    let mut j = i;
    while j < chars.len() && chars[j] == '$' {
        j += 1;
    }
    let start = j;
    while j < chars.len() && chars[j].is_ascii_alphabetic() {
        j += 1;
    }
    let after_letters = j;
    // Skip an absolute-row `$` marker between the column letters and the row digits.
    while j < chars.len() && chars[j] == '$' {
        j += 1;
    }
    let after_dollar = j;
    while j < chars.len() && chars[j].is_ascii_digit() {
        j += 1;
    }
    if after_letters == start || after_dollar == j {
        return None;
    }
    let letters: String = chars[start..after_letters].iter().collect::<String>().to_uppercase();
    let digits: String = chars[after_dollar..j].iter().collect();
    Some((j, format!("{letters}{digits}")))
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

    #[test]
    fn translates_simple_formulas_to_les() {
        assert_eq!(translate_excel_to_les("=A1+A2"), Some("=A1+A2".to_string()));
        assert_eq!(
            translate_excel_to_les("=SUM(A1:B2)"),
            Some("=SUM(RANGE(A1,B2))".to_string())
        );
        assert_eq!(
            translate_excel_to_les("=A1 + B2 : C3"),
            Some("=A1 + RANGE(B2,C3)".to_string())
        );
    }

    #[test]
    fn rejects_unsupported_formulas() {
        assert_eq!(translate_excel_to_les("=@A1"), None);
        assert_eq!(translate_excel_to_les("=Sheet2!A1"), None);
        assert_eq!(translate_excel_to_les("={1,2;3,4}"), None);
        assert_eq!(translate_excel_to_les(""), None);
        assert_eq!(
            translate_excel_to_les("=SUM($A$1:$B$2)"),
            Some("=SUM(RANGE(A1,B2))".to_string())
        );
    }

    #[test]
    fn iso_date_parsing() {
        assert_eq!(iso_to_date("2020-03-15").unwrap(), serial_from_ymd(2020, 3, 15));
        let noon = iso_to_date("2020-03-15T06:00:00").unwrap();
        assert!((noon - (serial_from_ymd(2020, 3, 15) + 0.25)).abs() < 1e-9);
        assert!(iso_to_date("not-a-date").is_none());
    }
}
