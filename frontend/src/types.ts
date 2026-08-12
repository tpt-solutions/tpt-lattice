// TypeScript mirrors of the JSON protocol spoken by `tpt-lattice-wasm`.
// Keep these in sync with `crates/tpt-lattice-wasm/src/lib.rs`.

// `LatticeError` is serialized as an externally-tagged enum. Unit variants
// become bare strings; newtype/struct variants become `{"Variant": payload}`.
export type LatticeError =
  | "DivByZero"
  | "NotANumber"
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
  | { type: "DeleteColumn"; index: number };

// Responses returned by the engine worker, matching the Rust `Response` enum.
export type Response =
  | { type: "Value"; value: CellValue }
  | { type: "Ok" }
  | { type: "Evaluated" }
  | { type: "OpsAccepted"; count: number }
  | { type: "Outbox"; ops: Op[] }
  | { type: "Cells"; cells: { cell: string; value: CellValue }[] }
  | { type: "Error"; message: string };

// ---- value helpers ---------------------------------------------------------

export function isError(v: CellValue): v is { Error: LatticeError } {
  return typeof v === "object" && v !== null && "Error" in v;
}

export function isEmpty(v: CellValue): boolean {
  return v === "Empty";
}

export function errorText(e: LatticeError): string {
  if (typeof e === "string") return e;
  const key = Object.keys(e)[0] as keyof typeof e;
  const payload = (e as Record<string, unknown>)[key];
  if (typeof payload === "string") return `${key}: ${payload}`;
  if (payload && typeof payload === "object") return `${key}: ${JSON.stringify(payload)}`;
  return key;
}

export function valueText(v: CellValue): string {
  if (v === "Empty") return "";
  if (typeof v === "object") {
    if ("Number" in v) return String(v.Number);
    if ("Text" in v) return v.Text;
    if ("Boolean" in v) return String(v.Boolean);
    if ("Error" in v) return `#${errorText(v.Error)}`;
  }
  return "";
}
