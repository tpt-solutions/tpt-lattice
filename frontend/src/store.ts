import type { CellValue } from "./types";
import type { Range } from "./grid/metrics";

export interface GridStore {
  /** Cached materialized values, keyed by `"col,row"`. */
  cells: Record<string, CellValue>;
  active: { col: number; row: number };
  selection: Range;
  editing: boolean;
  /** Bumped whenever `cells` changes, to drive canvas redraws. */
  rev: number;
}
