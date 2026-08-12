import { For, type Accessor } from "solid-js";
import type { CellValue } from "../types";
import { isError, valueText } from "../types";
import { toA1 } from "../grid/coords";
import type { Range } from "../grid/metrics";
import type { GridStore } from "../store";

export const VISUALLY_HIDDEN =
  "position:absolute;width:1px;height:1px;padding:0;margin:-1px;overflow:hidden;clip:rect(0 0 0 0);white-space:nowrap;border:0;";

export interface AccessibleGridProps {
  store: GridStore;
  formulas: Accessor<Record<string, string>>;
}

function cellKey(col: number, row: number) {
  return `${col},${row}`;
}

function inRange(c: number, r: number, range: Range) {
  return (
    c >= range.col0 && c <= range.col1 && r >= range.row0 && r <= range.row1
  );
}

/**
 * A parallel, screen-reader-only DOM mirror of the (canvas) grid. The grid itself
 * is drawn on a `<canvas>`, which exposes no per-cell semantics to assistive
 * technology; this hidden tree carries `role="grid"` / `role="row"` /
 * `role="gridcell"` so the document is navigable by AT and can be targeted with
 * `aria-activedescendant` from the canvas container.
 *
 * Only materialized cells (plus the active cell) are mirrored, which keeps the
 * tree small while still representing every cell a user can reach.
 */
export function AccessibleGrid(props: AccessibleGridProps) {
  const rows = () => {
    const set = new Set<number>();
    for (const k of Object.keys(props.store.cells)) {
      const r = Number(k.split(",")[1]);
      if (!Number.isNaN(r)) set.add(r);
    }
    set.add(props.store.active.row);
    return [...set].sort((a, b) => a - b);
  };

  const colsFor = (r: number) => {
    const set = new Set<number>();
    for (const k of Object.keys(props.store.cells)) {
      const [c, rr] = k.split(",").map(Number);
      if (rr === r) set.add(c);
    }
    set.add(props.store.active.col);
    return [...set].sort((a, b) => a - b);
  };

  const cellValue = (c: number, r: number): CellValue => {
    const k = cellKey(c, r);
    const f = props.formulas()[k];
    if (f !== undefined) return { Text: f };
    return props.store.cells[k] ?? "Empty";
  };

  const label = (c: number, r: number) => {
    const a1 = toA1(c, r);
    const v = cellValue(c, r);
    if (v === "Empty") return `${a1}, empty`;
    const text = valueText(v);
    const kind = isError(v) ? "error" : typeof v;
    return `${a1}, ${kind}: ${text}`;
  };

  return (
    <div role="grid" aria-label="Spreadsheet" style={VISUALLY_HIDDEN}>
      <For each={rows()}>
        {(r) => (
          <div role="row">
            <For each={colsFor(r)}>
              {(c) => {
                const a1 = toA1(c, r);
                const selected = inRange(c, r, props.store.selection);
                const active =
                  props.store.active.col === c && props.store.active.row === r;
                return (
                  <div
                    id={`cell-${a1}`}
                    role="gridcell"
                    aria-selected={selected}
                    aria-label={label(c, r)}
                    style={active ? "font-weight:bold;" : undefined}
                  >
                    {valueText(cellValue(c, r))}
                  </div>
                );
              }}
            </For>
          </div>
        )}
      </For>
    </div>
  );
}
