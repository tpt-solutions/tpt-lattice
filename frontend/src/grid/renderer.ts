import type { CellValue, CellStyle, LatticeError } from "../types";
import { isError, errorCode, formatSerialDate } from "../types";
import { columnLabel, parseA1 } from "./coords";
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
  /** Remote presence cursors to overlay: `{ actor, cell, color }[]`. */
  cursors?: { actor: number; cell: string; color: string }[];
  widths: number[];
  heights: number[];
  /** Number of leading columns that stay fixed while scrolling (0 = none). */
  freezeCols?: number;
  /** Number of leading rows that stay fixed while scrolling (0 = none). */
  freezeRows?: number;
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
  return errorCode(e);
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
    if ("Date" in v) return formatSerialDate(v.Date);
    if ("List" in v) return v.List.map((x) => valueToText(x, style)).join(", ");
    if ("Error" in v) return errorToText(v.Error);
  }
  return "";
}

export interface ClipRect {
  x: number;
  y: number;
  w: number;
  h: number;
}

/**
 * Draw the full grid into the canvas for the current viewport, honouring frozen
 * leading rows/columns. The viewport is split into four clipped zones (top-left,
 * top-right, bottom-left, bottom-right) so frozen cells stay pinned while the
 * scrollable zones move with the scroll offset.
 */
export function drawGrid(s: RenderState) {
  const { ctx, width, height, dpr, scrollX, scrollY, range } = s;

  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, width, height);
  ctx.fillStyle = COLORS.bg;
  ctx.fillRect(0, 0, width, height);

  const freezeCols = s.freezeCols ?? 0;
  const freezeRows = s.freezeRows ?? 0;
  // Screen-space edge of the frozen strip (content space for col/row 0 is HEADER_W/HEADER_H).
  const freezeEdgeX = freezeCols > 0 ? colX(freezeCols, s.widths) : HEADER_W;
  const freezeEdgeY = freezeRows > 0 ? rowY(freezeRows, s.heights) : HEADER_H;

  const inSel = (c: number, r: number) =>
    c >= s.selection.col0 &&
    c <= s.selection.col1 &&
    r >= s.selection.row0 &&
    r <= s.selection.row1;

  const drawCol1 = range.col1;
  const drawRow1 = range.row1;

  ctx.textBaseline = "middle";

  // Paint one zone. `frozenCol`/`frozenRow` decide whether an axis is pinned
  // (no scroll offset) or scrolls with the viewport.
  const paint = (
    c0: number,
    c1: number,
    r0: number,
    r1: number,
    frozenCol: boolean,
    frozenRow: boolean,
    clip: ClipRect,
  ) => {
    ctx.save();
    ctx.beginPath();
    ctx.rect(clip.x, clip.y, clip.w, clip.h);
    ctx.clip();

    for (let r = r0; r <= r1; r++) {
      const y = frozenRow ? rowY(r, s.heights) : rowY(r, s.heights) - scrollY;
      const h = rowHeight(r, s.heights);
      if (y + h < clip.y || y > clip.y + clip.h) continue;
      for (let c = c0; c <= c1; c++) {
        const x = frozenCol ? colX(c, s.widths) : colX(c, s.widths) - scrollX;
        const w = colWidth(c, s.widths);
        if (x + w < clip.x || x > clip.x + clip.w) continue;

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
        ctx.fillRect(x, y, w, h);

        const text = valueToText(v, style);
        // Non-color error cue: a warning glyph so errors are not conveyed by
        // the red background alone (accessibility: color-blind users).
        if (errored) drawErrorIcon(ctx, x + 4, y + h / 2);
        if (text) {
          ctx.fillStyle = errored ? COLORS.errorText : COLORS.text;
          ctx.font = `${style?.italic ? "italic " : ""}${style?.bold ? "700" : "400"} 13px ui-monospace, SFMono-Regular, Menlo, monospace`;
          const align = style?.align ?? (typeof v === "object" && "Number" in v ? "right" : "left");
          ctx.textAlign = align;
          const padX = 6 + (errored ? 14 : 0);
          const tx =
            align === "right"
              ? x + w - 6
              : align === "center"
                ? x + w / 2
                : x + padX;
          ctx.fillText(text, tx, y + h / 2);
        }

        // remote-change border
        if (remote) {
          ctx.strokeStyle = COLORS.remoteBorder;
          ctx.lineWidth = 2;
          ctx.strokeRect(x + 1, y + 1, w - 2, h - 2);
        }

        if (c === s.active.col && r === s.active.row && !s.editing) {
          ctx.strokeStyle = COLORS.selectionBorder;
          ctx.lineWidth = 2;
          ctx.strokeRect(x + 1, y + 1, w - 2, h - 2);
        }
      }
    }

    // grid lines (confined to the zone)
    ctx.strokeStyle = COLORS.gridLine;
    ctx.lineWidth = 1;
    ctx.beginPath();
    for (let c = c0; c <= c1 + 1; c++) {
      const x = Math.round(frozenCol ? colX(c, s.widths) : colX(c, s.widths) - scrollX) + 0.5;
      ctx.moveTo(x, clip.y);
      ctx.lineTo(x, clip.y + clip.h);
    }
    for (let r = r0; r <= r1 + 1; r++) {
      const y = Math.round(frozenRow ? rowY(r, s.heights) : rowY(r, s.heights) - scrollY) + 0.5;
      ctx.moveTo(clip.x, y);
      ctx.lineTo(clip.x + clip.w, y);
    }
    ctx.stroke();

    ctx.restore();
  };

  // Four zones; frozen zones are skipped when nothing is frozen.
  if (freezeCols > 0 && freezeRows > 0) {
    paint(0, freezeCols - 1, 0, freezeRows - 1, true, true, {
      x: HEADER_W,
      y: HEADER_H,
      w: freezeEdgeX - HEADER_W,
      h: freezeEdgeY - HEADER_H,
    });
  }
  if (freezeRows > 0) {
    paint(freezeCols, drawCol1, 0, freezeRows - 1, false, true, {
      x: freezeEdgeX,
      y: HEADER_H,
      w: width - freezeEdgeX,
      h: freezeEdgeY - HEADER_H,
    });
  }
  if (freezeCols > 0) {
    paint(0, freezeCols - 1, freezeRows, drawRow1, true, false, {
      x: HEADER_W,
      y: freezeEdgeY,
      w: freezeEdgeX - HEADER_W,
      h: height - freezeEdgeY,
    });
  }
  paint(freezeCols, drawCol1, freezeRows, drawRow1, false, false, {
    x: freezeEdgeX,
    y: freezeEdgeY,
    w: width - freezeEdgeX,
    h: height - freezeEdgeY,
  });

  // --- header gutters ---
  ctx.fillStyle = COLORS.headerBg;
  ctx.fillRect(0, 0, width, HEADER_H);
  ctx.fillRect(0, 0, HEADER_W, height);

  ctx.fillStyle = COLORS.headerText;
  ctx.font = "12px system-ui, sans-serif";
  ctx.textBaseline = "middle";
  ctx.textAlign = "center";

  // Column headers: frozen slice pinned, scrollable slice under the top-right zone.
  if (freezeCols > 0) {
    ctx.save();
    ctx.beginPath();
    ctx.rect(HEADER_W, 0, freezeEdgeX - HEADER_W, HEADER_H);
    ctx.clip();
    for (let c = 0; c < freezeCols; c++) {
      ctx.fillText(columnLabel(c), colX(c, s.widths) + colWidth(c, s.widths) / 2, HEADER_H / 2);
    }
    ctx.restore();
  }
  ctx.save();
  ctx.beginPath();
  ctx.rect(freezeEdgeX, 0, width - freezeEdgeX, HEADER_H);
  ctx.clip();
  for (let c = freezeCols; c <= drawCol1; c++) {
    const x = colX(c, s.widths) - scrollX + colWidth(c, s.widths) / 2;
    if (x < freezeEdgeX) continue;
    ctx.fillText(columnLabel(c), x, HEADER_H / 2);
  }
  ctx.restore();

  // Row headers: frozen slice pinned, scrollable slice under the bottom-left zone.
  if (freezeRows > 0) {
    ctx.save();
    ctx.beginPath();
    ctx.rect(0, HEADER_H, HEADER_W, freezeEdgeY - HEADER_H);
    ctx.clip();
    for (let r = 0; r < freezeRows; r++) {
      ctx.fillText(String(r + 1), HEADER_W / 2, rowY(r, s.heights) + rowHeight(r, s.heights) / 2);
    }
    ctx.restore();
  }
  ctx.save();
  ctx.beginPath();
  ctx.rect(0, freezeEdgeY, HEADER_W, height - freezeEdgeY);
  ctx.clip();
  for (let r = freezeRows; r <= drawRow1; r++) {
    const y = rowY(r, s.heights) - scrollY + rowHeight(r, s.heights) / 2;
    if (y < freezeEdgeY) continue;
    ctx.fillText(String(r + 1), HEADER_W / 2, y);
  }
  ctx.restore();

  // corner
  ctx.fillStyle = COLORS.headerBg;
  ctx.fillRect(0, 0, HEADER_W, HEADER_H);

  // freeze divider lines
  if (freezeCols > 0) {
    ctx.strokeStyle = "#9ca3af";
    ctx.lineWidth = 2;
    ctx.beginPath();
    ctx.moveTo(freezeEdgeX, HEADER_H);
    ctx.lineTo(freezeEdgeX, height);
    ctx.stroke();
  }
  if (freezeRows > 0) {
    ctx.strokeStyle = "#9ca3af";
    ctx.lineWidth = 2;
    ctx.beginPath();
    ctx.moveTo(HEADER_W, freezeEdgeY);
    ctx.lineTo(width, freezeEdgeY);
    ctx.stroke();
  }

  // --- remote presence cursors ---------------------------------------------
  if (s.cursors && s.cursors.length) {
    ctx.save();
    ctx.beginPath();
    ctx.rect(HEADER_W, HEADER_H, width - HEADER_W, height - HEADER_H);
    ctx.clip();
    for (const cur of s.cursors) {
      const p = parseA1(cur.cell);
      if (!p) continue;
      const frozenCol = p.col < freezeCols;
      const frozenRow = p.row < freezeRows;
      const x = frozenCol ? colX(p.col, s.widths) : colX(p.col, s.widths) - scrollX;
      const y = frozenRow ? rowY(p.row, s.heights) : rowY(p.row, s.heights) - scrollY;
      if (x < HEADER_W || y < HEADER_H || x > width || y > height) continue;
      const h = rowHeight(p.row, s.heights);
      drawCursor(ctx, x, y, h, cur.color, String(cur.actor));
    }
    ctx.restore();
  }
}

/** Draw a small colored triangle at a cell's top-left corner plus the actor label. */
function drawCursor(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  h: number,
  color: string,
  label: string,
) {
  const s = Math.max(8, Math.min(12, h));
  ctx.save();
  ctx.beginPath();
  ctx.moveTo(x, y);
  ctx.lineTo(x + s, y);
  ctx.lineTo(x, y + s);
  ctx.closePath();
  ctx.fillStyle = color;
  ctx.fill();
  ctx.strokeStyle = "#fff";
  ctx.lineWidth = 1;
  ctx.stroke();

  ctx.font = "bold 10px system-ui, sans-serif";
  const tagW = Math.max(16, ctx.measureText(label).width + 8);
  const tagX = x + s;
  const tagY = Math.min(y + s, y + h - 14);
  ctx.fillStyle = color;
  ctx.fillRect(tagX, tagY, tagW, 14);
  ctx.fillStyle = "#fff";
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  ctx.fillText(label, tagX + tagW / 2, tagY + 7);
  ctx.restore();
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
