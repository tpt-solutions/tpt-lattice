//! # tpt-lattice-export-xlsx
//!
//! Write a TPT Lattice [`SerializableGrid`](tpt_lattice_io::SerializableGrid) out
//! to a legacy `.xlsx` (OOXML) workbook. The writer authors the minimal set of
//! package parts directly (no heavy XML dependency) and round-trips through the
//! companion [`tpt-lattice-import-xlsx`] crate for testing.
//!
//! ```
//! use tpt_lattice_core::{CellId, CellValue};
//! use tpt_lattice_io::SerializableGrid;
//! use tpt_lattice_export_xlsx::export_to_bytes;
//!
//! let mut g = SerializableGrid::new();
//! g.set(CellId::from_a1("A1"), CellValue::Number(42.0));
//! let bytes = export_to_bytes(&g).unwrap();
//! assert!(!bytes.is_empty());
//! ```
//!
//! The produced workbook has a single worksheet (`Sheet1`). Numbers, text,
//! booleans, and source formulas are all carried through; `CellValue::Error`
//! values are written as their textual form.

use std::collections::BTreeSet;
use std::io::{Cursor, Write};

use tpt_lattice_core::CellId;
use tpt_lattice_io::SerializableGrid;
use zip::write::FileOptions;
use zip::ZipWriter;

/// Errors that can occur while exporting a workbook.
#[derive(Debug)]
pub enum ExportError {
    /// An I/O or zip-container failure.
    Io(String),
}

impl std::fmt::Display for ExportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExportError::Io(e) => write!(f, "xlsx export error: {e}"),
        }
    }
}

impl std::error::Error for ExportError {}

impl From<std::io::Error> for ExportError {
    fn from(e: std::io::Error) -> Self {
        ExportError::Io(e.to_string())
    }
}

impl From<zip::result::ZipError> for ExportError {
    fn from(e: zip::result::ZipError) -> Self {
        ExportError::Io(e.to_string())
    }
}

/// Serialize `grid` to raw `.xlsx` bytes.
pub fn export_to_bytes(grid: &SerializableGrid) -> Result<Vec<u8>, ExportError> {
    let mut buf: Cursor<Vec<u8>> = Cursor::new(Vec::new());
    {
        let mut zip = ZipWriter::new(&mut buf);
        let opts = FileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);

        zip.start_file("[Content_Types].xml", opts)?;
        zip.write_all(CONTENT_TYPES.as_bytes())?;

        zip.start_file("_rels/.rels", opts)?;
        zip.write_all(ROOT_RELS.as_bytes())?;

        zip.start_file("xl/workbook.xml", opts)?;
        zip.write_all(WORKBOOK.as_bytes())?;

        zip.start_file("xl/_rels/workbook.xml.rels", opts)?;
        zip.write_all(WORKBOOK_RELS.as_bytes())?;

        zip.start_file("xl/styles.xml", opts)?;
        zip.write_all(STYLES.as_bytes())?;

        let sheet = build_worksheet(grid);
        zip.start_file("xl/worksheets/sheet1.xml", opts)?;
        zip.write_all(sheet.as_bytes())?;

        zip.finish()?;
    }
    Ok(buf.into_inner())
}

/// Build the `xl/worksheets/sheet1.xml` body for `grid`. Cells are emitted in
/// row-major order (matching the `SerializableGrid` key ordering), grouped into
/// `<row>` elements keyed by their 1-based row index. A cell is emitted if it has
/// a value, a formula, or both.
fn build_worksheet(grid: &SerializableGrid) -> String {
    let mut ids: BTreeSet<u64> = BTreeSet::new();
    for (bits, _) in grid.iter() {
        ids.insert(bits.to_bits());
    }
    for (bits, _) in grid.iter_formulas() {
        ids.insert(bits.to_bits());
    }

    let mut rows: Vec<(u64, Vec<String>)> = Vec::new();
    let mut current_row: Option<u64> = None;
    let mut cells: Vec<String> = Vec::new();

    let flush = |r: u64, cs: &[String], out: &mut Vec<(u64, Vec<String>)>| {
        if !cs.is_empty() {
            out.push((r, cs.to_vec()));
        }
    };

    for bits in ids {
        let id = CellId::from_bits(bits);
        let r = id.row();
        if current_row != Some(r) {
            if let Some(cr) = current_row {
                flush(cr, &cells, &mut rows);
            }
            cells.clear();
            current_row = Some(r);
        }
        let v = grid.get(id);
        cells.push(cell_xml(id, Some(&v), grid.get_formula(id)));
    }
    if let Some(cr) = current_row {
        flush(cr, &cells, &mut rows);
    }

    let mut body = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>"#,
    );
    for (r, cs) in rows {
        body.push_str(&format!(r#"<row r="{}">"#, r + 1));
        for c in cs {
            body.push_str(&c);
        }
        body.push_str("</row>");
    }
    body.push_str("</sheetData></worksheet>");
    body
}

/// Render a single `<c>` element for `id`. Prefers the source formula when one is
/// present (writing it as an Excel `<f>` element, minus the leading `=`), and
/// falls back to the materialized value.
fn cell_xml(id: CellId, value: Option<&tpt_lattice_core::CellValue>, formula: Option<&str>) -> String {
    let ref_ = id.to_a1();
    if let Some(f) = formula {
        let f = f.strip_prefix('=').unwrap_or(f);
        let mut s = format!(r#"<c r="{ref_}"><f>{esc}</f>"#, ref_ = ref_, esc = xml_escape(f));
        // Include a cached value when we have one so the file opens with computed
        // numbers even before recalculation.
        if let Some(v) = value {
            if let Some(cached) = value_inner(v) {
                s.push_str(&format!("<v>{cached}</v>"));
            }
        }
        s.push_str("</c>");
        return s;
    }
    match value {
        Some(tpt_lattice_core::CellValue::Empty) | None => String::new(),
        Some(v) => {
            let (attrs, content) = cell_parts(v);
            format!(
                r#"<c r="{ref_}"{attrs}>{content}</c>"#,
                ref_ = ref_,
                attrs = attrs,
                content = content
            )
        }
    }
}

/// The `(type-attribute, inner-xml)` pair for a value. The attribute is empty for
/// numbers (the default cell type) and set for booleans/text-like cells.
fn cell_parts(v: &tpt_lattice_core::CellValue) -> (&'static str, String) {
    match v {
        tpt_lattice_core::CellValue::Empty => ("", String::new()),
        tpt_lattice_core::CellValue::Number(n) => ("", format!("<v>{n}</v>")),
        tpt_lattice_core::CellValue::Boolean(b) => {
            (r#" t="b""#, format!("<v>{}</v>", if *b { 1 } else { 0 }))
        }
        tpt_lattice_core::CellValue::Text(_)
        | tpt_lattice_core::CellValue::Error(_)
        | tpt_lattice_core::CellValue::Date(_)
        | tpt_lattice_core::CellValue::List(_) => (
            r#" t="inlineStr""#,
            format!(r#"<is><t>{esc}</t></is>"#, esc = xml_escape(&value_text(v))),
        ),
    }
}

/// The cached `<v>` content for a number/boolean cell, or `None` for string-like
/// cells (which are written as inline strings instead).
fn value_inner(v: &tpt_lattice_core::CellValue) -> Option<String> {
    match v {
        tpt_lattice_core::CellValue::Number(n) => Some(format!("<v>{n}</v>")),
        tpt_lattice_core::CellValue::Boolean(b) => Some(format!("<v>{}</v>", if *b { 1 } else { 0 })),
        _ => None,
    }
}

fn value_text(v: &tpt_lattice_core::CellValue) -> String {
    match v {
        tpt_lattice_core::CellValue::Text(s) => s.clone(),
        tpt_lattice_core::CellValue::Number(n) => n.to_string(),
        tpt_lattice_core::CellValue::Boolean(b) => b.to_string(),
        tpt_lattice_core::CellValue::Date(d) => d.to_string(),
        tpt_lattice_core::CellValue::List(items) => {
            let parts: Vec<String> = items.iter().map(value_text).collect();
            format!("[{}]", parts.join(", "))
        }
        tpt_lattice_core::CellValue::Empty => String::new(),
        tpt_lattice_core::CellValue::Error(e) => format!("#{e:?}"),
    }
}

fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>
  <Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>
  <Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/>
</Types>"#;

const ROOT_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>
</Relationships>"#;

const WORKBOOK: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
  <sheets>
    <sheet name="Sheet1" sheetId="1" r:id="rId1"/>
  </sheets>
</workbook>"#;

const WORKBOOK_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>
  <Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>
</Relationships>"#;

const STYLES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
  <fonts count="1"><font><sz val="11"/><name val="Calibri"/></font></fonts>
  <fills count="2"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill></fills>
  <borders count="1"><border/></borders>
  <cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs>
  <cellXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/></cellXfs>
</styleSheet>"#;

#[cfg(test)]
mod tests {
    use std::io::Read;
    use super::*;
    use tpt_lattice_core::{CellValue, LatticeError};

    #[test]
    fn exports_valid_package() {
        let mut g = SerializableGrid::new();
        g.set(CellId::from_a1("A1"), CellValue::Number(42.0));
        g.set(CellId::from_a1("B2"), CellValue::Text("hi".into()));
        g.set(CellId::from_a1("C3"), CellValue::Boolean(true));
        g.set_formula(CellId::from_a1("D4"), "=A1 * 2");
        g.set(
            CellId::from_a1("E5"),
            CellValue::Error(LatticeError::DivByZero),
        );

        let bytes = export_to_bytes(&g).unwrap();
        assert!(!bytes.is_empty());

        // The container must contain the expected OOXML parts and the values must
        // appear in the worksheet (verified by reading the zip back).
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let names: Vec<String> = zip.file_names().map(str::to_string).collect();
        assert!(names.contains(&"[Content_Types].xml".to_string()));
        assert!(names.contains(&"xl/worksheets/sheet1.xml".to_string()));

        let mut sheet = zip.by_name("xl/worksheets/sheet1.xml").unwrap();
        let mut bytes = Vec::new();
        sheet.read_to_end(&mut bytes).unwrap();
        let xml = String::from_utf8_lossy(&bytes);
        assert!(xml.contains(r#"r="A1""#) && xml.contains("<v>42</v>"));
        assert!(xml.contains(r#"r="B2""#) && xml.contains("hi"));
        assert!(xml.contains(r#"r="C3""#) && xml.contains(r#"t="b""#));
        assert!(xml.contains(r#"r="D4""#) && xml.contains("<f>A1 * 2</f>"));
        assert!(xml.contains(r#"r="E5""#));
    }

    #[test]
    fn empty_grid_still_writes_shell() {
        let g = SerializableGrid::new();
        let bytes = export_to_bytes(&g).unwrap();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        assert!(zip.by_name("xl/worksheets/sheet1.xml").is_ok());
    }
}
