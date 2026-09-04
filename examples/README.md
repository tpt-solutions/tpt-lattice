# Example templates

Ready-to-load grids for the TPT Lattice frontend. Load them from the UI with
**Toolbar → Open** (or any tool that speaks the LES grid format).

## Format

A grid file is JSON with two optional objects, both keyed by A1 cell addresses:

```json
{
  "name": "Optional display name",
  "cells": {
    "A1": { "Number": 42 },
    "B1": { "Text": "hello" },
    "C1": { "Boolean": true }
  },
  "formulas": {
    "D1": "=A1 * 2"
  }
}
```

- `cells` — literal cell values using the `CellValue` shape
  (`Number`, `Text`, `Boolean`, `Error`, `Date`, `List`).
- `formulas` — LES formula strings (without the leading `=`, i.e. the body only)
  keyed by the cell they belong to. On load, formulas override any literal value in
  the same cell.

See [`budget.json`](./templates/budget.json) and
[`project-tracker.json`](./templates/project-tracker.json) for complete examples.
