//! Direct OOXML (`.xlsx` zip) parsing for the bits `calamine` does not surface:
//! merged-cell regions and per-cell styling (bold, italic, alignment, number
//! format). calamine already yields the cell *values*; this module reads the raw
//! worksheet and `styles.xml` so the importer can carry styling through.
//!
//! The parsing is intentionally tolerant: a malformed or partial part yields an
//! empty result for that part rather than failing the whole import.

use std::collections::{BTreeMap, HashMap};
use std::io::{Cursor, Read};

use quick_xml::events::Event;
use quick_xml::reader::Reader;
use zip::ZipArchive;

use tpt_lattice_core::CellId;

use crate::ImportedSheet;

/// A rectangular merged region. Only the top-left cell normally carries a value;
/// the remaining spanned cells are visually joined to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MergedRegion {
    pub top_left: CellId,
    pub bottom_right: CellId,
}

/// Horizontal text alignment of a cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HorizontalAlign {
    Left,
    Center,
    Right,
    Fill,
    Justify,
    CenterContinuous,
    Distributed,
}

/// Vertical text alignment of a cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerticalAlign {
    Top,
    Center,
    Bottom,
    Justify,
    Distributed,
}

/// A subset of Excel cell styling that LES can currently represent.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CellStyle {
    pub bold: bool,
    pub italic: bool,
    pub horizontal_align: Option<HorizontalAlign>,
    pub vertical_align: Option<VerticalAlign>,
    pub number_format: Option<String>,
    /// Foreground fill color as an ARGB hex string (e.g. `FF00FF00`), if any.
    pub fill_color: Option<String>,
    /// Font (text) color as an ARGB hex string, if any.
    pub font_color: Option<String>,
    /// Font face name (e.g. `Calibri`), if present.
    pub font_name: Option<String>,
    /// Whether the cell has a (non-default) border applied.
    pub border: bool,
}

// ---------------------------------------------------------------------------
// Public entry points
// ---------------------------------------------------------------------------

/// Enrich `sheet` with the merged-cell regions and per-cell styles found in the
/// raw `bytes` for the given `sheet_name`. Best-effort: any part that cannot be
/// read is skipped, leaving the corresponding fields empty.
pub(crate) fn attach_styles_and_merges(
    bytes: &[u8],
    sheet_name: &str,
    sheet: &mut ImportedSheet,
) {
    if let Some(styles_xml) = read_styles_xml(bytes) {
        let table = parse_styles(&styles_xml);
        if let Some(path) = worksheet_path(bytes, sheet_name) {
            if let Some(xml) = read_entry(bytes, &path) {
                let (merged, cell_styles) = parse_worksheet(&xml);
                sheet.merged_cells = merged;
                sheet.styles = cell_styles
                    .into_iter()
                    .filter_map(|(cell, idx)| {
                        build_style(&table, idx).map(|s| (cell, s))
                    })
                    .collect();
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Zip helpers
// ---------------------------------------------------------------------------

fn read_entry(bytes: &[u8], path: &str) -> Option<String> {
    let mut zip = ZipArchive::new(Cursor::new(bytes.to_vec())).ok()?;
    let mut file = zip.by_name(path).ok()?;
    let mut out = String::new();
    file.read_to_string(&mut out).ok()?;
    Some(out)
}

fn read_styles_xml(bytes: &[u8]) -> Option<String> {
    read_entry(bytes, "xl/styles.xml")
}

/// Resolve a sheet name to its workbook-relative zip path by reading
/// `xl/workbook.xml` (sheet name -> r:id) and `xl/_rels/workbook.xml.rels`
/// (r:id -> target worksheet file).
fn worksheet_path(bytes: &[u8], sheet_name: &str) -> Option<String> {
    let wb = read_entry(bytes, "xl/workbook.xml")?;
    let name_to_rid = parse_sheet_name_to_rid(&wb);
    let rid = name_to_rid.get(sheet_name)?;

    let rels = read_entry(bytes, "xl/_rels/workbook.xml.rels").unwrap_or_default();
    let rid_to_target = parse_rid_to_target(&rels);
    let target = rid_to_target.get(rid)?;
    Some(normalize_target(target))
}

/// Targets in workbook rels are relative to the `xl/` directory
/// (e.g. `worksheets/sheet1.xml`); make them absolute package paths.
fn normalize_target(target: &str) -> String {
    let t = target.trim();
    if t.starts_with('/') {
        t.trim_start_matches('/').to_string()
    } else if let Some(stripped) = t.strip_prefix("xl/") {
        stripped.to_string()
    } else {
        format!("xl/{t}")
    }
}

// ---------------------------------------------------------------------------
// Workbook / relationship parsing
// ---------------------------------------------------------------------------

fn parse_sheet_name_to_rid(xml: &str) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    while let Ok(event) = reader.read_event_into(&mut buf) {
        if let Event::Start(e) | Event::Empty(e) = &event {
            if e.name().as_ref() == "sheet" {
                let mut name = None;
                let mut rid = None;
                for a in e.attributes().flatten() {
                    match a.key.as_ref() {
                        "name" => name = Some(a.value.into_owned()),
                        "r:id" | "rId" => rid = Some(a.value.into_owned()),
                        _ => {}
                    }
                }
                if let (Some(n), Some(r)) = (name, rid) {
                    map.insert(n, r);
                }
            }
        } else if matches!(event, Event::Eof) {
            break;
        }
        buf.clear();
    }
    map
}

fn parse_rid_to_target(xml: &str) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    while let Ok(event) = reader.read_event_into(&mut buf) {
        if let Event::Start(e) | Event::Empty(e) = &event {
            if e.name().as_ref() == "Relationship" {
                let mut id = None;
                let mut target = None;
                for a in e.attributes().flatten() {
                    match a.key.as_ref() {
                        "Id" | "id" => id = Some(a.value.into_owned()),
                        "Target" | "target" => target = Some(a.value.into_owned()),
                        _ => {}
                    }
                }
                if let (Some(i), Some(t)) = (id, target) {
                    map.insert(i, t);
                }
            }
        } else if matches!(event, Event::Eof) {
            break;
        }
        buf.clear();
    }
    map
}

// ---------------------------------------------------------------------------
// Worksheet parsing: merged cells + per-cell style indices
// ---------------------------------------------------------------------------

fn parse_worksheet(
    xml: &str,
) -> (Vec<MergedRegion>, BTreeMap<CellId, u32>) {
    let mut merged = Vec::new();
    let mut styles: BTreeMap<CellId, u32> = BTreeMap::new();
    let mut in_merge = false;
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    while let Ok(event) = reader.read_event_into(&mut buf) {
        match &event {
            Event::Start(e) | Event::Empty(e) => {
                let qn = e.name();
                let name = qn.as_ref();
                let is_empty = matches!(event, Event::Empty(_));
                match name {
                    "mergeCells" => in_merge = true,
                    "mergeCell" if in_merge => {
                        for a in e.attributes().flatten() {
                            if a.key.as_ref() == "ref" {
                                if let Some(mr) = parse_merge_ref(&a.value) {
                                    merged.push(mr);
                                }
                            }
                        }
                    }
                    "c" => {
                        let mut r: Option<String> = None;
                        let mut s: Option<u32> = None;
                        for a in e.attributes().flatten() {
                            match a.key.as_ref() {
                                "r" => r = Some(a.value.into_owned()),
                                "s" => s = parse_u32(&a.value),
                                _ => {}
                            }
                        }
                        if let (Some(r), Some(s)) = (r, s) {
                            if let Ok(id) = CellId::try_from_a1(&r) {
                                styles.insert(id, s);
                            }
                        }
                    }
                    _ => {}
                }
                if is_empty && name == "mergeCells" {
                    in_merge = false;
                }
            }
            Event::End(e) => {
                if e.name().as_ref() == "mergeCells" {
                    in_merge = false;
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    (merged, styles)
}

/// Parse an Excel range ref such as `A1:C3` into a [`MergedRegion`].
fn parse_merge_ref(v: &str) -> Option<MergedRegion> {
    let (a, b) = v.split_once(':')?;
    let top_left = CellId::try_from_a1(a.trim()).ok()?;
    let bottom_right = CellId::try_from_a1(b.trim()).ok()?;
    Some(MergedRegion { top_left, bottom_right })
}

// ---------------------------------------------------------------------------
// styles.xml parsing
// ---------------------------------------------------------------------------

#[derive(Default)]
struct FontStyle {
    bold: bool,
    italic: bool,
    color: Option<String>,
    name: Option<String>,
}

#[derive(Default)]
struct Xf {
    font_id: u32,
    num_fmt_id: u32,
    fill_id: u32,
    border_id: u32,
    horizontal: Option<HorizontalAlign>,
    vertical: Option<VerticalAlign>,
}

struct StyleTable {
    fonts: Vec<FontStyle>,
    cell_xfs: Vec<Xf>,
    num_fmts: HashMap<u32, String>,
    /// ARGB fill color per fill index (index 0/1 are the default none/gray125).
    fills: Vec<Option<String>>,
}

fn parse_styles(xml: &str) -> StyleTable {
    let mut fonts: Vec<FontStyle> = Vec::new();
    let mut cur_font: Option<FontStyle> = None;
    let mut in_fonts = false;

    let mut cell_xfs: Vec<Xf> = Vec::new();
    let mut cur_xf: Option<Xf> = None;
    let mut in_cellxfs = false;

    let mut num_fmts: HashMap<u32, String> = HashMap::new();
    let mut cur_numfmt: Option<(u32, String)> = None;
    let mut in_numfmts = false;

    let mut fills: Vec<Option<String>> = Vec::new();
    let mut cur_fill: Option<Option<String>> = None;
    let mut in_fills = false;

    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    while let Ok(event) = reader.read_event_into(&mut buf) {
        let is_empty = matches!(event, Event::Empty(_));
        match &event {
            Event::Start(e) | Event::Empty(e) => {
                let qn = e.name();
                let name = qn.as_ref();
                match name {
                    "fonts" => in_fonts = true,
                    "font" if in_fonts => cur_font = Some(FontStyle::default()),
                    "b" if cur_font.is_some() => cur_font.as_mut().unwrap().bold = true,
                    "i" if cur_font.is_some() => cur_font.as_mut().unwrap().italic = true,
                    "color" if cur_font.is_some() => {
                        for a in e.attributes().flatten() {
                            if a.key.as_ref() == "rgb" {
                                cur_font.as_mut().unwrap().color = Some(a.value.into_owned());
                            }
                        }
                    }
                    "name" if cur_font.is_some() => {
                        for a in e.attributes().flatten() {
                            if a.key.as_ref() == "val" {
                                cur_font.as_mut().unwrap().name = Some(a.value.into_owned());
                            }
                        }
                    }

                    "cellXfs" => in_cellxfs = true,
                    "xf" if in_cellxfs => {
                        let mut xf = Xf::default();
                        for a in e.attributes().flatten() {
                            match a.key.as_ref() {
                                "fontId" => xf.font_id = parse_u32(&a.value).unwrap_or(0),
                                "numFmtId" => xf.num_fmt_id = parse_u32(&a.value).unwrap_or(0),
                                "fillId" => xf.fill_id = parse_u32(&a.value).unwrap_or(0),
                                "borderId" => xf.border_id = parse_u32(&a.value).unwrap_or(0),
                                _ => {}
                            }
                        }
                        cur_xf = Some(xf);
                    }
                    "alignment" if cur_xf.is_some() => {
                        let xf = cur_xf.as_mut().unwrap();
                        for a in e.attributes().flatten() {
                            match a.key.as_ref() {
                                "horizontal" => xf.horizontal = parse_halign(&a.value),
                                "vertical" => xf.vertical = parse_valign(&a.value),
                                _ => {}
                            }
                        }
                    }

                    "fills" => in_fills = true,
                    "fill" if in_fills => {
                        if is_empty {
                            fills.push(None);
                        } else {
                            cur_fill = Some(None);
                        }
                    }
                    "fgColor" if in_fills => {
                        for a in e.attributes().flatten() {
                            if a.key.as_ref() == "rgb" {
                                if let Some(f) = cur_fill.as_mut() {
                                    *f = Some(a.value.into_owned());
                                }
                            }
                        }
                    }
                    "numFmts" => in_numfmts = true,
                    "numFmt" if in_numfmts => {
                        let mut id = None;
                        let mut code = None;
                        for a in e.attributes().flatten() {
                            match a.key.as_ref() {
                                "numFmtId" => id = parse_u32(&a.value),
                                "formatCode" => code = Some(a.value.into_owned()),
                                _ => {}
                            }
                        }
                        if let (Some(id), Some(code)) = (id, code) {
                            cur_numfmt = Some((id, code));
                        }
                    }
                    _ => {}
                }

                // Self-closing variants need to be finalized inline.
                if is_empty && name == "xf" && in_cellxfs {
                    if let Some(x) = cur_xf.take() {
                        cell_xfs.push(x);
                    }
                }
                if is_empty && name == "numFmt" && in_numfmts {
                    if let Some((id, code)) = cur_numfmt.take() {
                        num_fmts.insert(id, code);
                    }
                }
                if is_empty && name == "font" && in_fonts {
                    if let Some(f) = cur_font.take() {
                        fonts.push(f);
                    }
                }
            }
            Event::End(e) => {
                let qn = e.name();
                let name = qn.as_ref();
                match name {
                    "fonts" => in_fonts = false,
                    "font" if in_fonts => {
                        if let Some(f) = cur_font.take() {
                            fonts.push(f);
                        }
                    }
                    "cellXfs" => in_cellxfs = false,
                    "xf" if in_cellxfs => {
                        if let Some(x) = cur_xf.take() {
                            cell_xfs.push(x);
                        }
                    }
                    "numFmts" => in_numfmts = false,
                    "numFmt" if in_numfmts => {
                        if let Some((id, code)) = cur_numfmt.take() {
                            num_fmts.insert(id, code);
                        }
                    }
                    "fills" => in_fills = false,
                    "fill" if in_fills => {
                        if let Some(f) = cur_fill.take() {
                            fills.push(f);
                        }
                    }
                    _ => {}
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    StyleTable { fonts, cell_xfs, num_fmts, fills }
}

fn build_style(table: &StyleTable, idx: u32) -> Option<CellStyle> {
    let xf = table.cell_xfs.get(idx as usize)?;
    let font = table.fonts.get(xf.font_id as usize);
    let number_format = if xf.num_fmt_id == 0 {
        None
    } else {
        table
            .num_fmts
            .get(&xf.num_fmt_id)
            .cloned()
            .or_else(|| builtin_numfmt(xf.num_fmt_id))
    };
    let fill_color = table.fills.get(xf.fill_id as usize).and_then(|f| f.clone());
    Some(CellStyle {
        bold: font.map(|f| f.bold).unwrap_or(false),
        italic: font.map(|f| f.italic).unwrap_or(false),
        horizontal_align: xf.horizontal,
        vertical_align: xf.vertical,
        number_format,
        fill_color,
        font_color: font.and_then(|f| f.color.clone()),
        font_name: font.and_then(|f| f.name.clone()),
        border: xf.border_id != 0,
    })
}

/// A handful of the most common built-in number-format ids, so that a styled
/// cell carries a recognizable format even when no custom `numFmt` is present.
fn builtin_numfmt(id: u32) -> Option<String> {
    Some(
        match id {
            1 => "0",
            2 => "0.00",
            3 => "#,##0",
            4 => "#,##0.00",
            9 => "0%",
            10 => "0.00%",
            11 => "0.00E+00",
            14 => "mm-dd-yy",
            15 => "d-mmm-yy",
            16 => "d-mmm",
            20 => "h:mm",
            21 => "h:mm:ss",
            22 => "m/d/yy h:mm",
            _ => return None,
        }
        .to_string(),
    )
}

// ---------------------------------------------------------------------------
// Small attribute/value helpers
// ---------------------------------------------------------------------------

fn parse_u32(v: &str) -> Option<u32> {
    v.parse().ok()
}

fn parse_halign(v: &str) -> Option<HorizontalAlign> {
    match v.to_ascii_lowercase().as_str() {
        "left" => Some(HorizontalAlign::Left),
        "center" => Some(HorizontalAlign::Center),
        "right" => Some(HorizontalAlign::Right),
        "fill" => Some(HorizontalAlign::Fill),
        "justify" => Some(HorizontalAlign::Justify),
        "centercontinuous" => Some(HorizontalAlign::CenterContinuous),
        "distributed" => Some(HorizontalAlign::Distributed),
        _ => None,
    }
}

fn parse_valign(v: &str) -> Option<VerticalAlign> {
    match v.to_ascii_lowercase().as_str() {
        "top" => Some(VerticalAlign::Top),
        "center" => Some(VerticalAlign::Center),
        "bottom" => Some(VerticalAlign::Bottom),
        "justify" => Some(VerticalAlign::Justify),
        "distributed" => Some(VerticalAlign::Distributed),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STYLES: &str = r#"<?xml version="1.0"?>
<styleSheet>
  <numFmts count="1"><numFmt numFmtId="164" formatCode="0.00"/></numFmts>
  <fonts count="3">
    <font><b/></font>
    <font><i/></font>
    <font><b/><i/></font>
  </fonts>
  <cellXfs count="3">
    <xf fontId="0" numFmtId="0"/>
    <xf fontId="1" numFmtId="2"><alignment horizontal="center"/></xf>
    <xf fontId="2" numFmtId="164"><alignment vertical="top"/></xf>
  </cellXfs>
</styleSheet>"#;

    const SHEET: &str = r#"<?xml version="1.0"?>
<worksheet>
  <mergeCells count="1"><mergeCell ref="B2:D4"/></mergeCells>
  <sheetData>
    <row r="1"><c r="A1" s="0"/><c r="B1" s="1"/></row>
    <row r="2"><c r="C2" s="2"/></row>
  </sheetData>
</worksheet>"#;

    #[test]
    fn parses_styles_and_merges() {
        let table = parse_styles(STYLES);
        assert!(table.fonts[0].bold && !table.fonts[0].italic);
        assert!(table.fonts[1].italic && !table.fonts[1].bold);
        assert!(table.fonts[2].bold && table.fonts[2].italic);

        assert_eq!(table.cell_xfs[1].horizontal, Some(HorizontalAlign::Center));
        assert_eq!(table.cell_xfs[2].vertical, Some(VerticalAlign::Top));
        assert_eq!(table.num_fmts.get(&164), Some(&"0.00".to_string()));

        let (merged, styles) = parse_worksheet(SHEET);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].top_left, CellId::from_a1("B2"));
        assert_eq!(merged[0].bottom_right, CellId::from_a1("D4"));

        let s1 = build_style(&table, styles[&CellId::from_a1("B1")]);
        assert_eq!(s1.unwrap().horizontal_align, Some(HorizontalAlign::Center));
        let s2 = build_style(&table, styles[&CellId::from_a1("C2")]);
        assert_eq!(s2.unwrap().number_format, Some("0.00".to_string()));
    }

    #[test]
    fn merge_ref_parsing() {
        let mr = parse_merge_ref("A1:C3").unwrap();
        assert_eq!(mr.top_left, CellId::from_a1("A1"));
        assert_eq!(mr.bottom_right, CellId::from_a1("C3"));
        assert!(parse_merge_ref("A1").is_none());
    }

    #[test]
    fn parses_extended_styles() {
        const STYLE_XML: &str = r#"<?xml version="1.0"?>
        <styleSheet>
          <numFmts count="1"><numFmt numFmtId="164" formatCode="0.00"/></numFmts>
          <fonts count="2">
            <font><b/><color rgb="FFFF0000"/><name val="Consolas"/></font>
            <font/>
          </fonts>
          <fills count="3">
            <fill><patternFill patternType="none"/></fill>
            <fill><patternFill patternType="gray125"/></fill>
            <fill><patternFill patternType="solid"><fgColor rgb="FF00FF00"/></patternFill></fill>
          </fills>
          <borders count="2"><border><left/></border><border/></borders>
          <cellXfs count="2">
            <xf fontId="0" numFmtId="164" fillId="2" borderId="1"/>
            <xf fontId="1" numFmtId="0" fillId="0" borderId="0"/>
          </cellXfs>
        </styleSheet>"#;
        let table = parse_styles(STYLE_XML);
        assert_eq!(table.fills.len(), 3);
        assert_eq!(table.fills[2], Some("FF00FF00".to_string()));
        let s = build_style(&table, 0).unwrap();
        assert!(s.bold);
        assert_eq!(s.font_color, Some("FFFF0000".to_string()));
        assert_eq!(s.font_name, Some("Consolas".to_string()));
        assert_eq!(s.fill_color, Some("FF00FF00".to_string()));
        assert_eq!(s.number_format, Some("0.00".to_string()));
        assert!(s.border);
        assert!(!build_style(&table, 1).unwrap().border);
    }
}
