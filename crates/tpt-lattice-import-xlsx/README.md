# tpt-lattice-import-xlsx

Opt-in translation of legacy `.xlsx` workbooks into
[TPT Lattice](https://github.com/tpt-solutions/tpt-lattice) primitives, built on
[`calamine`](https://docs.rs/calamine).

Unsupported Excel formulas are not silently dropped or broken: they become explicit
`CellValue::Error(LatticeError::UnsupportedFormula)` values so downstream math stays correct.

## Features

- Import the first worksheet (`import_first_sheet`) or a named sheet (`import_sheet`).
- Map Excel cell types to `CellValue`: ints/floats → `Number`, strings → `Text`,
  booleans → `Boolean`, datetimes → `Text`, cell errors → `LatticeError::RefError`.
- Excel formulas surface as `LatticeError::UnsupportedFormula`.
- Best-effort named-range capture (`name → first referenced CellId`).
- Merged-cell regions (`ImportedSheet::merged_cells`) parsed directly from the OOXML.
- Basic per-cell styles (`ImportedSheet::styles`): bold, italic, horizontal/vertical
  alignment, and number formats (built-in or custom `numFmt` codes).
- Graceful failure (no panics) on malformed input.

## Installation

```toml
[dependencies]
tpt-lattice-import-xlsx = "0.1.0"
```

## Usage

```rust
use tpt_lattice_import_xlsx::import_first_sheet;
use std::fs;

let bytes = fs::read("workbook.xlsx").unwrap();
let sheet = import_first_sheet(&bytes).expect("failed to import");
for (id, value) in &sheet.cells {
    println!("{id}: {value:?}");
}
```

`ImportError` distinguishes a `Calamine` parsing failure from a `SheetNotFound` error.

## Caveats

- Only the first worksheet is imported by `import_first_sheet`; use `import_sheet` for others.
- Excel formulas are not evaluated; they are flagged as unsupported rather than translated.
- Style extraction covers a basic subset (bold, italic, alignment, number format). Rich
  formatting (fills, borders, fonts beyond bold/italic) is not represented.

## License

Licensed under either of [MIT](../../LICENSE-MIT) or [Apache-2.0](../../LICENSE-APACHE) at your
option.
