//! Integration tests that import a real `.xlsx` fixture produced by
//! `tests/generate_fixture.py` (run `py tests/generate_fixture.py` to rebuild it).

use tpt_lattice_core::{CellId, CellValue, LatticeError};
use tpt_lattice_import_xlsx::{import_first_sheet, import_sheet, HorizontalAlign};

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/sample.xlsx");

#[test]
fn imports_numbers_text_and_booleans() {
    let sheet = import_first_sheet(include_bytes!("../tests/fixtures/sample.xlsx"))
        .expect("fixture should import");

    assert_eq!(
        sheet.cells.get(&CellId::from_a1("A1")),
        Some(&CellValue::Number(1.0))
    );
    assert_eq!(
        sheet.cells.get(&CellId::from_a1("A2")),
        Some(&CellValue::Number(2.0))
    );
    assert_eq!(
        sheet.cells.get(&CellId::from_a1("A3")),
        Some(&CellValue::Number(3.5))
    );
    assert_eq!(
        sheet.cells.get(&CellId::from_a1("B1")),
        Some(&CellValue::Text("hello".into()))
    );
    assert_eq!(
        sheet.cells.get(&CellId::from_a1("C1")),
        Some(&CellValue::Boolean(true))
    );
    assert_eq!(
        sheet.cells.get(&CellId::from_a1("C2")),
        Some(&CellValue::Boolean(false))
    );
}

#[test]
fn formulas_become_unsupported_error() {
    let sheet = import_first_sheet(include_bytes!("../tests/fixtures/sample.xlsx")).unwrap();
    let d1 = sheet.cells.get(&CellId::from_a1("D1")).expect("D1 present");
    assert_eq!(d1, &CellValue::Error(LatticeError::unsupported("=A1+A2")));
}

#[test]
fn named_ranges_are_captured() {
    let sheet = import_first_sheet(include_bytes!("../tests/fixtures/sample.xlsx")).unwrap();
    assert_eq!(
        sheet.named_ranges.get("MyRange"),
        Some(&CellId::from_a1("A1"))
    );
}

#[test]
fn merged_cells_are_imported() {
    let sheet = import_first_sheet(include_bytes!("../tests/fixtures/sample.xlsx")).unwrap();
    assert!(
        sheet.merged_cells.iter().any(|m| {
            m.top_left == CellId::from_a1("B5") && m.bottom_right == CellId::from_a1("C6")
        }),
        "expected merged region B5:C6, got {:?}",
        sheet.merged_cells
    );
}

#[test]
fn basic_styles_are_imported() {
    let sheet = import_first_sheet(include_bytes!("../tests/fixtures/sample.xlsx")).unwrap();

    let a5 = sheet.styles.get(&CellId::from_a1("A5"));
    assert!(a5.map(|s| s.bold).unwrap_or(false), "A5 should be bold");

    let a6 = sheet.styles.get(&CellId::from_a1("A6"));
    assert!(a6.map(|s| s.italic).unwrap_or(false), "A6 should be italic");

    let a7 = sheet.styles.get(&CellId::from_a1("A7"));
    assert_eq!(
        a7.and_then(|s| s.number_format.as_deref()),
        Some("0.00"),
        "A7 should carry the 0.00 number format"
    );

    let a8 = sheet.styles.get(&CellId::from_a1("A8"));
    assert_eq!(
        a8.and_then(|s| s.horizontal_align),
        Some(HorizontalAlign::Center),
        "A8 should be horizontally centered"
    );
}

#[test]

fn importing_by_name_matches_first_sheet() {
    let bytes = include_bytes!("../tests/fixtures/sample.xlsx");
    let first = import_first_sheet(bytes).unwrap();
    let named = import_sheet(bytes, "Sheet1").unwrap();
    assert_eq!(first.cells, named.cells);
    assert_eq!(first.named_ranges, named.named_ranges);
}

// `FIXTURE` is referenced so the compile-time path is checked even though we use
// `include_bytes!` for the actual bytes (keeps the test hermetic and offline).
#[test]
fn fixture_path_is_well_formed() {
    assert!(FIXTURE.ends_with("sample.xlsx"));
}
