import type { CellValue, LatticeError } from "../types";
import { isError } from "../types";
import { columnLabel } from "./coords";
import { COL_W, HEADER_H, HEADER_W, ROW_H, type Range } from "./metrics";

export interface RenderState {
  ctx: CanvasRenderingContext2D;
  width: number; // CSS pixels (content viewport size)
  height: number;
  dpr: number;
  scrollX: number;
  scrollY: number;
  range: Range;
  cells: Map<string, CellValue>; // keyed by "col,row"
  active: { col: number; row: number };
  selection: Range;
  editing: boolean;
}

const COLORS = {
  bg: "#ffffff",
  headerBg: "#f3f4f6",
  gridLine: "#e5e7eb",
  headerText: "#6b7280",
  text: "#111827",
  selection: "rgba(59, 130, 246, 0.18)",
  selectionBorder: "#2563eb",
  errorBg: "#fee2e2",
  errorText: "#b91c1c",
};

function a1Key(col: number, row: number) {
  return `${col},${row}`;
}

function errorToText(e: LatticeError): string {
  if (typeof e === "string") return e;
  const key = Object.keys(e)[0] as keyof typeof e;
  const p = (e as Record<string, unknown>)[key];
  if (typeof p === "string") return `${key}: ${p}`;
  if (p && typeof p === "object") return `${key}: ${JSON.stringify(p)}`;
  return key;
}

function valueToText(v: CellValue): string {
  if (v === "Empty") return "";
  if (typeof v === "object") {
    if ("Number" in v) return String(v.Number);
    if ("Text" in v) return v.Text;
    if ("Boolean" in v) return String(v.Boolean);
    if ("Error" in v) return `#${errorToText(v.Error)}`;
  }
  return "";
}

/** Draw the full grid into the canvas for the current viewport. */
export function drawGrid(s: RenderState) {
  const { ctx, width, height, dpr, scrollX, scrollY, range } = s;

  // Reset transform and clear in device pixels.
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, width, height);
  ctx.fillStyle = COLORS.bg;
  ctx.fillRect(0, 0, width, height);

  const inSel = (c: number, r: number) =>
    c >= s.selection.col0 &&
    c <= s.selection.col1 &&
    r >= s.selection.row0 &&
    r <= s.selection.row1;

  // --- cells ---
  ctx.textBaseline = "middle";
  ctx.font = "13px ui-monospace, SFMono-Regular, Menlo, monospace";

  for (let r = range.row0; r <= range.row1; r++) {
    const y = HEADER_H + r * ROW_H - scrollY;
    if (y + ROW_H < HEADER_H || y > height) continue;
    for (let c = range.col0; c <= range.col1; c++) {
      const x = HEADER_W + c * COL_W - scrollX;
      if (x + COL_W < HEADER_W || x > width) continue;

      const v = s.cells.get(a1Key(c, r)) ?? "Empty";
      const selected = inSel(c, r);
      const errored = isError(v);

      // background
      if (errored) ctx.fillStyle = COLORS.errorBg;
      else if (selected && !s.editing) ctx.fillStyle = COLORS.selection;
      else ctx.fillStyle = COLORS.bg;
      ctx.fillRect(x, y, COL_W, ROW_H);

      // content
      const text = valueToText(v);
      if (text) {
        ctx.fillStyle = errored ? COLORS.errorText : COLORS.text;
        ctx.textAlign = typeof v === "object" && "Number" in v ? "right" : "left";
        const padX = 6;
        const tx = ctx.textAlign === "right" ? x + COL_W - padX : x + padX;
        ctx.fillText(text, tx, y + ROW_H / 2);
      }
    }
  }

  // --- vertical + horizontal grid lines ---
  ctx.strokeStyle = COLORS.gridLine;
  ctx.lineWidth = 1;
  ctx.beginPath();
  for (let c = range.col0; c <= range.col1 + 1; c++) {
    const x = Math.round(HEADER_W + c * COL_W - scrollX) + 0.5;
    if (x < HEADER_W - 1) continue;
    ctx.moveTo(x, HEADER_H);
    ctx.lineTo(x, height);
  }
  for (let r = range.row0; r <= range.row1 + 1; r++) {
    const y = Math.round(HEADER_H + r * ROW_H - scrollY) + 0.5;
    if (y < HEADER_H - 1) continue;
    ctx.moveTo(HEADER_W, y);
    ctx.lineTo(width, y);
  }
  ctx.stroke();

  // --- header gutters ---
  ctx.fillStyle = COLORS.headerBg;
  ctx.fillRect(0, 0, width, HEADER_H);
  ctx.fillRect(0, 0, HEADER_W, height);

  ctx.fillStyle = COLORS.headerText;
  ctx.font = "12px system-ui, sans-serif";
  ctx.textBaseline = "middle";
  ctx.textAlign = "center";
  // column headers
  for (let c = range.col0; c <= range.col1; c++) {
    const x = HEADER_W + c * COL_W - scrollX + COL_W / 2;
    if (x < HEADER_W) continue;
    ctx.fillText(columnLabel(c), x, HEADER_H / 2);
  }
  // row headers
  ctx.textBaseline = "middle";
  for (let r = range.row0; r <= range.row1; r++) {
    const y = HEADER_H + r * ROW_H - scrollY + ROW_H / 2;
    if (y < HEADER_H) continue;
    ctx.fillText(String(r + 1), HEADER_W / 2, y);
  }

  // corner
  ctx.fillStyle = COLORS.headerBg;
  ctx.fillRect(0, 0, HEADER_W, HEADER_H);

  // --- active-cell border ---
  if (!s.editing) {
    const ax = HEADER_W + s.active.col * COL_W - scrollX;
    const ay = HEADER_H + s.active.row * ROW_H - scrollY;
    ctx.strokeStyle = COLORS.selectionBorder;
    ctx.lineWidth = 2;
    ctx.strokeRect(ax + 1, ay + 1, COL_W - 2, ROW_H - 2);
  }
}
