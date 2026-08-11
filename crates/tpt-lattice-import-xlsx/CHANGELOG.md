# Changelog

All notable changes to this crate are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2026-08-11

### Added
- `ImportError` enum (`Calamine`, `SheetNotFound`) with `Display` / `Error` impls.
- `ImportedSheet` with `cells` and `named_ranges` maps.
- `import_first_sheet` and `import_sheet` backed by `calamine::Xlsx`.
- Cell-type mapping (`map_cell`) for ints, floats, strings, booleans, datetimes, and errors.
- Excel formula detection → `LatticeError::UnsupportedFormula`.
- Best-effort named-range capture via `defined_names`.
- Graceful-failure test on malformed input.
