// TypeScript mirrors of the JSON protocol spoken by `tpt-lattice-wasm`.
// Keep these in sync with `crates/tpt-lattice-wasm/src/lib.rs`.

// `LatticeError` is serialized as an externally-tagged enum. Unit variants
// become bare strings; newtype/struct variants become `{"Variant": payload}`.
export type LatticeError =
  | "DivByZero"
  | "NotANumber"
  | "NA"
  | { ParseError: string }
  | { TypeError: { expected: string; got: string } }
  | { NameError: string }
  | { RefError: string }
  | { CircularReference: string }
  | { UnsupportedFormula: string }
  | { ArgumentError: string }
  | { Internal: string };

// `CellValue` is also externally tagged.
export type CellValue =
  | "Empty"
  | { Number: number }
  | { Text: string }
  | { Boolean: boolean }
  | { Date: number }
  | { List: CellValue[] }
  | { Error: LatticeError };

// Per-cell visual formatting. Client-side only (not yet part of the CRDT/sync
// model); it drives how a cell is rendered. All fields are optional and inherit
// the default when absent.
export type CellStyle = {
  bold?: boolean;
  italic?: boolean;
  align?: "left" | "center" | "right";
  numFmt?: "general" | "number" | "percent" | "currency";
};

// A vector clock is a map of actor id -> sequence number.
export type VectorClock = Record<number, number>;

// Mirrors the Rust `Op` enum (externally tagged). The `cell` field is the packed
// `CellId` bits (a number), not an A1 string — clients relay ops opaquely.
export type Op =
  | { SetCell: { cell: number; value: CellValue; clock: VectorClock; actor: number } }
  | { DeleteCell: { cell: number; clock: VectorClock; actor: number } }
  | { InsertRow: { id: string; after: string | null; clock: VectorClock; actor: number } }
  | { InsertColumn: { id: string; after: string | null; clock: VectorClock; actor: number } }
  | { DeleteRow: { id: string; clock: VectorClock; actor: number } }
  | { DeleteColumn: { id: string; clock: VectorClock; actor: number } };

// Requests sent to the engine worker. Externally tagged with a `"type"` field,
// matching `#[serde(tag = "type")]` on the Rust `Request` enum.
export type Request =
  | { type: "SetCell"; cell: string; value: CellValue }
  | { type: "SetFormula"; cell: string; formula: string }
  | { type: "GetCell"; cell: string }
  | { type: "Evaluate" }
  | { type: "ApplyOps"; ops: Op[] }
  | { type: "Reset" }
  | { type: "Init"; actor: number }
  | { type: "DeleteCell"; cell: string }
  | { type: "TakeOutbox" }
  | { type: "ListCells" }
  | { type: "InsertRow"; index: number | null }
  | { type: "DeleteRow"; index: number }
  | { type: "InsertColumn"; index: number | null }
  | { type: "DeleteColumn"; index: number }
  | { type: "NewSheet"; name: string }
  | { type: "DeleteSheet"; name: string }
  | { type: "RenameSheet"; from: string; to: string }
  | { type: "SelectSheet"; name: string }
  | { type: "ListSheets" }
  | { type: "GetGraph" }
  | { type: "SetNamed"; name: string; expr: string }
  | { type: "ClearNamed"; name: string }
  | { type: "ListNamed" }
  | { type: "Checkpoint"; label: string }
  | { type: "ListHistory" }
  | { type: "Restore"; index: number }
  | { type: "SaveVersion"; label: string }
  | { type: "ListVersions" }
  | { type: "Diff"; left: number; right: number }
  | { type: "Fork"; name: string }
  | { type: "MergeBranch"; name: string }
  | { type: "ListBranches" }
  | { type: "RegisterUDF"; name: string; bytes: number[] }
  | { type: "UnregisterUDF"; name: string }
  | { type: "ListUDFs" };

// Responses returned by the engine worker, matching the Rust `Response` enum.
export type Response =
  | { type: "Value"; value: CellValue }
  | { type: "Ok" }
  | { type: "Evaluated" }
  | { type: "OpsAccepted"; count: number }
  | { type: "Outbox"; ops: Op[] }
  | { type: "Cells"; cells: { cell: string; value: CellValue }[] }
  | { type: "Sheets"; sheets: string[]; active: string }
  | { type: "Graph"; nodes: string[]; edges: [string, string][] }
  | { type: "Named"; names: [string, string][] }
  | { type: "History"; entries: [number, string][] }
  | { type: "Versions"; entries: [number, string, string][] }
  | { type: "Diff"; rows: DiffRow[] }
  | { type: "Branches"; entries: [string, string][] }
  | { type: "Merge"; applied: number; conflicts: MergeConflict[] }
  | { type: "UDFs"; names: string[] }
  | { type: "Error"; message: string };

// ---- value helpers ---------------------------------------------------------

export function isError(v: CellValue): v is { Error: LatticeError } {
  return typeof v === "object" && v !== null && "Error" in v;
}

export function isEmpty(v: CellValue): boolean {
  return v === "Empty";
}

/// Map a [`LatticeError`] to its familiar Excel-style code (e.g. `#DIV/0!`),
/// mirroring the Rust `LatticeError::Display` impl. The payload of newtype/
/// struct variants is intentionally dropped so the on-screen code matches Excel.
export function errorCode(e: LatticeError): string {
  if (typeof e === "string") {
    switch (e) {
      case "DivByZero":
        return "#DIV/0!";
      case "NotANumber":
        return "#NUM!";
      case "NA":
        return "#N/A";
      default:
        return "#ERROR!";
    }
  }
  const key = Object.keys(e)[0];
  switch (key) {
    case "ParseError":
    case "UnsupportedFormula":
    case "ArgumentError":
    case "Internal":
      return "#ERROR!";
    case "TypeError":
      return "#VALUE!";
    case "NameError":
      return "#NAME?";
    case "RefError":
      return "#REF!";
    case "CircularReference":
      return "#CIRC!";
    default:
      return "#ERROR!";
  }
}

export function errorText(e: LatticeError): string {
  return errorCode(e);
}

export function valueText(v: CellValue): string {
  if (v === "Empty") return "";
  if (typeof v === "object") {
    if ("Number" in v) return String(v.Number);
    if ("Text" in v) return v.Text;
    if ("Boolean" in v) return String(v.Boolean);
    if ("Date" in v) return formatSerialDate(v.Date);
    if ("List" in v) return v.List.map(valueText).join(", ");
    if ("Error" in v) return errorCode(v.Error);
  }
  return "";
}

/// Render an Excel serial date (days since 1899-12-30) as `YYYY-MM-DD` (or
/// `YYYY-MM-DD HH:MM` when a time fraction is present). Mirrors the Rust
/// `format_serial_date` helper.
export function formatSerialDate(serial: number): string {
  const days = Math.floor(serial);
  // Unix epoch (1970-01-01) is Excel serial 25569.
  const unixDays = days - 25569;
  const jsDate = new Date(unixDays * 86400 * 1000);
  const y = jsDate.getUTCFullYear();
  const m = String(jsDate.getUTCMonth() + 1).padStart(2, "0");
  const d = String(jsDate.getUTCDate()).padStart(2, "0");
  const frac = serial - days;
  if (Math.abs(frac) < 1e-9) return `${y}-${m}-${d}`;
  const secs = Math.round(frac * 86400);
  const hh = String(Math.floor(secs / 3600) % 24).padStart(2, "0");
  const mm = String(Math.floor(secs / 60) % 60).padStart(2, "0");
  const ss = String(secs % 60).padStart(2, "0");
  return `${y}-${m}-${d} ${hh}:${mm}:${ss}`;
}

/// A cell's value + optional formula as shown in a diff / merge report.
/// A cell's value + optional formula as shown in a diff / merge report.
export type DiffCell = {
  value: CellValue | null;
  formula: string | null;
};

/// One row of a version diff.
export type DiffRow = {
  cell: string;
  status: "added" | "removed" | "changed" | "unchanged";
  left: DiffCell | null;
  right: DiffCell | null;
};

/// A cell that could not be auto-merged (both sides changed differently).
export type MergeConflict = {
  cell: string;
  base: DiffCell | null;
  ours: DiffCell | null;
  theirs: DiffCell | null;
};

