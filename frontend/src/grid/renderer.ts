import type { CellValue, CellStyle, LatticeError } from "../types";
import { isError } from "../types";
import { columnLabel } from "./coords";
import {
  colWidth,
  rowHeight,
  colX,
  rowY,
  HEADER_H,
  HEADER_W,
  type Range,
} from "./metrics";

export interface RenderState {
  ctx: CanvasRenderingContext2D;
  width: number; // CSS pixels (content viewport size)
  height: number;
  dpr: number;
  scrollX: number;
  scrollY: number;
  range: Range;
  cells: Map<string, CellValue>; // keyed by "col,row"
  styles: Map<string, CellStyle>;
  active: { col: number; row: number };
  selection: Range;
  editing: boolean;
  /** Cells changed by a remote peer since the last local edit (conflict UI). */
  remote?: Set<string>;
  /** Cells currently matching an active find query. */
  find?: Set<string>;
  widths: number[];
  heights: number[];
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
  remoteBg: "rgba(245, 158, 11, 0.28)",
  remoteBorder: "#d97706",
  findBg: "rgba(250, 204, 21, 0.45)",
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

/** Format a numeric value per the cell's number format. */
function formatNumber(n: number, style: CellStyle | undefined): string {
  const fmt = style?.numFmt ?? "general";
  switch (fmt) {
    case "number":
      return n.toLocaleString(undefined, { minimumFractionDigits: 2, maximumFractionDigits: 2 });
    case "percent":
      return `${(n * 100).toLocaleString(undefined, { maximumFractionDigits: 2 })}%`;
    case "currency":
      return n.toLocaleString(undefined, {
        style: "currency",
        currency: "USD",
        maximumFractionDigits: 2,
      });
    default:
      return String(n);
  }
}

function valueToText(v: CellValue, style: CellStyle | undefined): string {
  if (v === "Empty") return "";
  if (typeof v === "object") {
    if ("Number" in v) return formatNumber(v.Number, style);
    if ("Text" in v) return v.Text;
    if ("Boolean" in v) return String(v.Boolean);
    if ("Error" in v) return `#${errorToText(v.Error)}`;
  }
  return "";
}

/** Draw the full grid into the canvas for the current viewport. */
export function drawGrid(s: RenderState) {
  const { ctx, width, height, dpr, scrollX, scrollY, range } = s;

  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, width, height);
  ctx.fillStyle = COLORS.bg;
  ctx.fillRect(0, 0, width, height);

  const inSel = (c: number, r: number) =>
    c >= s.selection.col0 &&
    c <= s.selection.col1 &&
    r >= s.selection.row0 &&
    r <= s.selection.row1;

  ctx.textBaseline = "middle";

  for (let r = range.row0; r <= range.row1; r++) {
    const y = rowY(r, s.heights) - scrollY;
    if (y + rowHeight(r, s.heights) < HEADER_H || y > height) continue;
    for (let c = range.col0; c <= range.col1; c++) {
      const x = colX(c, s.widths) - scrollX;
      if (x + colWidth(c, s.widths) < HEADER_W || x > width) continue;

      const k = a1Key(c, r);
      const v = s.cells.get(k) ?? "Empty";
      const style = s.styles.get(k);
      const selected = inSel(c, r) && !s.editing;
      const errored = isError(v);
      const remote = s.remote?.has(k);
      const found = s.find?.has(k);

      // background (layered: selection < find < remote < error)
      if (errored) ctx.fillStyle = COLORS.errorBg;
      else if (remote) ctx.fillStyle = COLORS.remoteBg;
      else if (found) ctx.fillStyle = COLORS.findBg;
      else if (selected) ctx.fillStyle = COLORS.selection;
      else ctx.fillStyle = COLORS.bg;
      ctx.fillRect(x, y, colWidth(c, s.widths), rowHeight(r, s.heights));

      // content
      const text = valueToText(v, style);
      // Non-color error cue: a warning glyph so errors are not conveyed by
      // the red background alone (accessibility: color-blind users).
      if (errored) {
        drawErrorIcon(ctx, x + 4, y + rowHeight(r, s.heights) / 2);
      }
      if (text) {
        ctx.fillStyle = errored ? COLORS.errorText : COLORS.text;
        ctx.font = `${style?.italic ? "italic " : ""}${style?.bold ? "700" : "400"} 13px ui-monospace, SFMono-Regular, Menlo, monospace`;
        const align = style?.align ?? (typeof v === "object" && "Number" in v ? "right" : "left");
        ctx.textAlign = align;
        const padX = 6 + (errored ? 14 : 0);
        const tx =
          align === "right"
            ? x + colWidth(c, s.widths) - 6
            : align === "center"
              ? x + colWidth(c, s.widths) / 2
              : x + padX;
        ctx.fillText(text, tx, y + rowHeight(r, s.heights) / 2);
      }

      // remote-change border
      if (remote) {
        ctx.strokeStyle = COLORS.remoteBorder;
        ctx.lineWidth = 2;
        ctx.strokeRect(x + 1, y + 1, colWidth(c, s.widths) - 2, rowHeight(r, s.heights) - 2);
      }
    }
  }

  // --- grid lines ---
  ctx.strokeStyle = COLORS.gridLine;
  ctx.lineWidth = 1;
  ctx.beginPath();
  for (let c = range.col0; c <= range.col1 + 1; c++) {
    const x = Math.round(colX(c, s.widths) - scrollX) + 0.5;
    if (x < HEADER_W - 1) continue;
    ctx.moveTo(x, HEADER_H);
    ctx.lineTo(x, height);
  }
  for (let r = range.col0; r <= range.row1 + 1; r++) {
    const y = Math.round(rowY(r, s.heights) - scrollY) + 0.5;
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
  for (let c = range.col0; c <= range.col1; c++) {
    const x = colX(c, s.widths) - scrollX + colWidth(c, s.widths) / 2;
    if (x < HEADER_W) continue;
    ctx.fillText(columnLabel(c), x, HEADER_H / 2);
  }
  ctx.textBaseline = "middle";
  for (let r = range.row0; r <= range.row1; r++) {
    const y = rowY(r, s.heights) - scrollY + rowHeight(r, s.heights) / 2;
    if (y < HEADER_H) continue;
    ctx.fillText(String(r + 1), HEADER_W / 2, y);
  }

  // corner
  ctx.fillStyle = COLORS.headerBg;
  ctx.fillRect(0, 0, HEADER_W, HEADER_H);

  // --- active-cell border ---
  if (!s.editing) {
    const ax = colX(s.active.col, s.widths) - scrollX;
    const ay = rowY(s.active.row, s.heights) - scrollY;
    ctx.strokeStyle = COLORS.selectionBorder;
    ctx.lineWidth = 2;
    ctx.strokeRect(ax + 1, ay + 1, colWidth(s.active.col, s.widths) - 2, rowHeight(s.active.row, s.heights) - 2);
  }
}

/** Draw a small warning triangle at (cx, cy) as a non-color error cue. */
function drawErrorIcon(ctx: CanvasRenderingContext2D, cx: number, cy: number) {
  const s = 9;
  ctx.save();
  ctx.beginPath();
  ctx.moveTo(cx, cy - s / 2);
  ctx.lineTo(cx + s / 2, cy + s / 2);
  ctx.lineTo(cx - s / 2, cy + s / 2);
  ctx.closePath();
  ctx.fillStyle = "#b91c1c";
  ctx.fill();
  ctx.fillStyle = "#ffffff";
  ctx.font = "bold 8px system-ui, sans-serif";
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  ctx.fillText("!", cx, cy + 1);
  ctx.restore();
}
