import type { CellValue, CellStyle } from "./types";
import type { Range } from "./grid/metrics";

export interface GridStore {
  /** Cached materialized values, keyed by `"col,row"`. */
  cells: Record<string, CellValue>;
  /** Per-cell visual formatting, keyed by `"col,row"`. */
  styles: Record<string, CellStyle>;
  active: { col: number; row: number };
  selection: Range;
  editing: boolean;
  /** Bumped whenever `cells` changes, to drive canvas redraws. */
  rev: number;
}
