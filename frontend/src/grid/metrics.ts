// Grid geometry. Column/row sizes are variable (user-resizable); sizes default to
// the constants below and are overridden per-index by the `widths`/`heights` arrays
// passed through the renderer and grid. All pixel math is content-space (i.e. before
// the scroll offset is applied).
export const DEFAULT_COL_W = 96;
export const DEFAULT_ROW_H = 26;
export const HEADER_W = 52;
export const HEADER_H = 26;

export interface Range {
  col0: number;
  row0: number;
  col1: number; // inclusive
  row1: number; // inclusive
}

export function colWidth(col: number, widths: number[]): number {
  return widths[col] ?? DEFAULT_COL_W;
}

export function rowHeight(row: number, heights: number[]): number {
  return heights[row] ?? DEFAULT_ROW_H;
}

/** Pixel X of a column's left edge in content space. */
export function colX(col: number, widths: number[]): number {
  let x = HEADER_W;
  for (let c = 0; c < col; c++) x += colWidth(c, widths);
  return x;
}

/** Pixel Y of a row's top edge in content space. */
export function rowY(row: number, heights: number[]): number {
  let y = HEADER_H;
  for (let r = 0; r < row; r++) y += rowHeight(r, heights);
  return y;
}

/** Total content width up to (and including) `colCount` columns. */
export function totalWidth(colCount: number, widths: number[]): number {
  let w = HEADER_W;
  for (let c = 0; c < colCount; c++) w += colWidth(c, widths);
  return w;
}

/** Total content height up to (and including) `rowCount` rows. */
export function totalHeight(rowCount: number, heights: number[]): number {
  let h = HEADER_H;
  for (let r = 0; r < rowCount; r++) h += rowHeight(r, heights);
  return h;
}

/** Visible cell range for a given scroll offset and viewport size. */
export function visibleRange(
  scrollX: number,
  scrollY: number,
  width: number,
  height: number,
  buffer = 2,
  widths: number[] = [],
  heights: number[] = [],
): Range {
  let c = 0;
  while (c < 1_000_000 && colX(c, widths) + colWidth(c, widths) <= scrollX) c++;
  const col0 = Math.max(0, c - buffer);
  let cEnd = c;
  while (colX(cEnd, widths) < scrollX + width) cEnd++;
  const col1 = cEnd + buffer;

  let r = 0;
  while (r < 1_000_000 && rowY(r, heights) + rowHeight(r, heights) <= scrollY) r++;
  const row0 = Math.max(0, r - buffer);
  let rEnd = r;
  while (rowY(rEnd, heights) < scrollY + height) rEnd++;
  const row1 = rEnd + buffer;

  return { col0, row0, col1, row1 };
}

/** Column index at a canvas X (CSS px), or -1 if inside the row-header gutter. */
export function colAtCanvasX(x: number, scrollX: number, widths: number[]): number {
  if (x < HEADER_W) return -1;
  return colAtContentX(x + scrollX - HEADER_W, widths);
}

/** Row index at a canvas Y (CSS px), or -1 if inside the column-header gutter. */
export function rowAtCanvasY(y: number, scrollY: number, heights: number[]): number {
  if (y < HEADER_H) return -1;
  return rowAtContentY(y + scrollY - HEADER_H, heights);
}

/** Column index at a content-space X (distance from the left edge of column 0). */
export function colAtContentX(contentX: number, widths: number[]): number {
  let x = contentX;
  let c = 0;
  for (;;) {
    const w = colWidth(c, widths);
    if (x < w) return c;
    x -= w;
    c++;
  }
}

/** Row index at a content-space Y (distance from the top edge of row 0). */
export function rowAtContentY(contentY: number, heights: number[]): number {
  let y = contentY;
  let r = 0;
  for (;;) {
    const h = rowHeight(r, heights);
    if (y < h) return r;
    y -= h;
    r++;
  }
}

/**
 * Distance (in px) from the left edge of `col`'s header to its right boundary —
 * used to detect resize-handle hovers. Returns -1 when not within `tol` of the edge.
 */
export function colResizeHit(x: number, scrollX: number, widths: number[]): number {
  if (x < HEADER_W) return -1;
  let cx = x + scrollX - HEADER_W;
  let c = 0;
  for (;;) {
    const w = colWidth(c, widths);
    if (cx >= w - 4 && cx <= w + 4) return c;
    cx -= w;
    c++;
  }
}

export function rowResizeHit(y: number, scrollY: number, heights: number[]): number {
  if (y < HEADER_H) return -1;
  let cy = y + scrollY - HEADER_H;
  let r = 0;
  for (;;) {
    const h = rowHeight(r, heights);
    if (cy >= h - 4 && cy <= h + 4) return r;
    cy -= h;
    r++;
  }
}
