// Fixed grid geometry. A real implementation would track per-column/row sizes;
// fixed sizes keep the virtualization math simple and deterministic.
export const COL_W = 96;
export const ROW_H = 26;
export const HEADER_W = 52;
export const HEADER_H = 26;

export interface Range {
  col0: number;
  row0: number;
  col1: number; // inclusive
  row1: number; // inclusive
}

/** Visible cell range for a given scroll offset and viewport (content) size. */
export function visibleRange(
  scrollX: number,
  scrollY: number,
  width: number,
  height: number,
  buffer = 2,
): Range {
  const col0 = Math.max(0, Math.floor(scrollX / COL_W) - buffer);
  const row0 = Math.max(0, Math.floor(scrollY / ROW_H) - buffer);
  const col1 = Math.floor((scrollX + width) / COL_W) + buffer;
  const row1 = Math.floor((scrollY + height) / ROW_H) + buffer;
  return { col0, row0, col1, row1 };
}

/** Pixel X of a column's left edge in content space (before scroll offset). */
export function colX(col: number): number {
  return HEADER_W + col * COL_W;
}

/** Pixel Y of a row's top edge in content space. */
export function rowY(row: number): number {
  return HEADER_H + row * ROW_H;
}
