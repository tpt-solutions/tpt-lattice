// Pure keyboard-navigation logic for the grid, separated from the DOM event
// handler so it can be unit-tested without a browser. The handler in `Grid.tsx`
// calls [`keyboardNavigate`] and applies the resulting cell (or does nothing when
// it returns `null`, e.g. for editing/non-navigation keys).

export interface NavRequest {
  active: { col: number; row: number };
  /** `e.key` of the pressed key. */
  key: string;
  /** `e.ctrlKey` (also treats meta as equivalent for cross-platform). */
  ctrlKey: boolean;
  /** Number of rows visible in a page (PageUp/PageDown step). */
  pageRows: number;
  /** Returns true if a cell holds data (used for edge-jump navigation). */
  hasData: (col: number, row: number) => boolean;
  /** Furthest populated column/row, used by Ctrl+End. */
  lastCol: number;
  lastRow: number;
}

export interface NavResult {
  col: number;
  row: number;
}

/**
 * Compute the next active cell for a navigation key, or `null` if the key is not
 * a navigation key (e.g. Enter, a printable character). All movement is clamped
 * to row/col >= 0.
 */
export function keyboardNavigate(req: NavRequest): NavResult | null {
  const { active, key, ctrlKey, pageRows, hasData, lastCol, lastRow } = req;
  const a = active;

  if (key === "Home") {
    return ctrlKey ? { col: 0, row: 0 } : { col: 0, row: a.row };
  }
  if (key === "End") {
    if (ctrlKey) return { col: lastCol, row: lastRow };
    let c = a.col;
    while (hasData(c + 1, a.row)) c++;
    return { col: c, row: a.row };
  }
  if (key === "PageUp") return { col: a.col, row: Math.max(0, a.row - pageRows) };
  if (key === "PageDown") return { col: a.col, row: a.row + pageRows };

  if (ctrlKey) {
    switch (key) {
      case "ArrowUp": {
        let r = a.row;
        while (r > 0 && hasData(a.col, r - 1)) r--;
        return { col: a.col, row: r };
      }
      case "ArrowDown": {
        let r = a.row;
        while (hasData(a.col, r + 1)) r++;
        return { col: a.col, row: r };
      }
      case "ArrowLeft": {
        let c = a.col;
        while (c > 0 && hasData(c - 1, a.row)) c--;
        return { col: c, row: a.row };
      }
      case "ArrowRight": {
        let c = a.col;
        while (hasData(c + 1, a.row)) c++;
        return { col: c, row: a.row };
      }
    }
    return null;
  }

  switch (key) {
    case "ArrowUp":
      return { col: a.col, row: Math.max(0, a.row - 1) };
    case "ArrowDown":
      return { col: a.col, row: a.row + 1 };
    case "ArrowLeft":
      return { col: Math.max(0, a.col - 1), row: a.row };
    case "ArrowRight":
      return { col: a.col + 1, row: a.row };
    default:
      return null;
  }
}
