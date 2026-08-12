import { Accessor, createEffect, onCleanup, onMount } from "solid-js";
import type { SetStoreFunction } from "solid-js/store";
import type { GridStore } from "../store";
import { CellEditor } from "./CellEditor";
import { drawGrid } from "./renderer";
import {
  HEADER_H,
  HEADER_W,
  DEFAULT_ROW_H,
  colAtCanvasX,
  rowAtCanvasY,
  colResizeHit,
  rowResizeHit,
  colX,
  rowY,
  colWidth,
  rowHeight,
  type Range,
} from "./metrics";

export interface GridProps {
  store: GridStore;
  setStore: SetStoreFunction<GridStore>;
  bumpRev: () => void;
  scrollX: Accessor<number>;
  scrollY: Accessor<number>;
  setScrollX: (v: number) => void;
  setScrollY: (v: number) => void;
  viewW: Accessor<number>;
  viewH: Accessor<number>;
  setView: (w: number, h: number) => void;
  beginEdit: (initial?: string) => void;
  commitEdit: (text: string) => void;
  cancelEdit: () => void;
  setActiveCell: (col: number, row: number) => void;
  setSelection: (range: Range) => void;
  extendSelection: (col: number, row: number) => void;
  editInitial: Accessor<string>;
  widths: Accessor<number[]>;
  heights: Accessor<number[]>;
  setColWidth: (col: number, w: number) => void;
  setRowHeight: (row: number, h: number) => void;
  onContextMenu: (clientX: number, clientY: number, col: number, row: number) => void;
  remote: Accessor<Set<string>>;
  find: Accessor<Set<string>>;
}

const BIG = 1_000_000;

export function Grid(props: GridProps) {
  let container!: HTMLDivElement;
  let canvas!: HTMLCanvasElement;
  let dragging = false;
  let headerDrag: { kind: "col" | "row"; anchor: number } | null = null;
  let resizing: { kind: "col" | "row"; index: number; start: number; size: number } | null = null;

  const cellKey = (c: number, r: number) => `${c},${r}`;
  const hasData = (c: number, r: number) =>
    (props.store.cells[cellKey(c, r)] ?? "Empty") !== "Empty";

  const cellFromEvent = (e: MouseEvent) => {
    const rect = canvas.getBoundingClientRect();
    const x = e.clientX - rect.left;
    const y = e.clientY - rect.top;
    const col = colAtCanvasX(x, props.scrollX(), props.widths());
    const row = rowAtCanvasY(y, props.scrollY(), props.heights());
    if (col < 0 || row < 0) return null;
    return { col, row };
  };

  onMount(() => {
    const ro = new ResizeObserver(() => {
      props.setView(container.clientWidth, container.clientHeight);
    });
    ro.observe(container);
    props.setView(container.clientWidth, container.clientHeight);
    onCleanup(() => ro.disconnect());
  });

  // Redraw whenever anything visible changes.
  createEffect(() => {
    const dpr = window.devicePixelRatio || 1;
    const w = props.viewW();
    const h = props.viewH();
    canvas.width = Math.max(1, Math.floor(w * dpr));
    canvas.height = Math.max(1, Math.floor(h * dpr));
    canvas.style.width = `${w}px`;
    canvas.style.height = `${h}px`;

    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    void props.store.rev; // track cell updates
    void props.store.styles; // track formatting changes
    void props.widths(); // track resize
    void props.heights();
    void props.remote();
    void props.find();
    drawGrid({
      ctx,
      width: w,
      height: h,
      dpr,
      scrollX: props.scrollX(),
      scrollY: props.scrollY(),
      range: props.store.selection,
      cells: new Map(Object.entries(props.store.cells)),
      styles: new Map(Object.entries(props.store.styles)),
      active: props.store.active,
      selection: props.store.selection,
      editing: props.store.editing,
      remote: props.remote(),
      find: props.find(),
      widths: props.widths(),
      heights: props.heights(),
    });
  });

  const onWheel = (e: WheelEvent) => {
    e.preventDefault();
    props.setScrollX(Math.max(0, props.scrollX() + e.deltaX));
    props.setScrollY(Math.max(0, props.scrollY() + e.deltaY));
  };

  const onMouseDown = (e: MouseEvent) => {
    const rect = canvas.getBoundingClientRect();
    const x = e.clientX - rect.left;
    const y = e.clientY - rect.top;

    // Resize handles take priority.
    const rc = colResizeHit(x, props.scrollX(), props.widths());
    if (rc >= 0) {
      resizing = { kind: "col", index: rc, start: e.clientX, size: colWidth(rc, props.widths()) };
      e.preventDefault();
      return;
    }
    const rr = rowResizeHit(y, props.scrollY(), props.heights());
    if (rr >= 0) {
      resizing = { kind: "row", index: rr, start: e.clientY, size: rowHeight(rr, props.heights()) };
      e.preventDefault();
      return;
    }

    // Header gutters -> column/row selection.
    if (y < HEADER_H && x >= HEADER_W) {
      const col = colAtCanvasX(x, props.scrollX(), props.widths());
      headerDrag = { kind: "col", anchor: col };
      props.setSelection({ col0: col, row0: 0, col1: col, row1: BIG });
      props.setActiveCell(col, 0);
      return;
    }
    if (x < HEADER_W && y >= HEADER_H) {
      const row = rowAtCanvasY(y, props.scrollY(), props.heights());
      headerDrag = { kind: "row", anchor: row };
      props.setSelection({ col0: 0, row0: row, col1: BIG, row1: row });
      props.setActiveCell(0, row);
      return;
    }

    const cell = cellFromEvent(e);
    if (!cell) return;
    dragging = true;
    props.setActiveCell(cell.col, cell.row);
  };

  const onMouseMove = (e: MouseEvent) => {
    const rect = canvas.getBoundingClientRect();
    const x = e.clientX - rect.left;
    const y = e.clientY - rect.top;

    if (resizing) {
      if (resizing.kind === "col") {
        const w = Math.max(24, resizing.size + (e.clientX - resizing.start));
        props.setColWidth(resizing.index, w);
      } else {
        const h = Math.max(16, resizing.size + (e.clientY - resizing.start));
        props.setRowHeight(resizing.index, h);
      }
      return;
    }

    if (headerDrag) {
      if (headerDrag.kind === "col") {
        const col = colAtCanvasX(x, props.scrollX(), props.widths());
        const a = headerDrag.anchor;
        props.setSelection({
          col0: Math.min(a, col),
          row0: 0,
          col1: Math.max(a, col),
          row1: BIG,
        });
      } else {
        const row = rowAtCanvasY(y, props.scrollY(), props.heights());
        const a = headerDrag.anchor;
        props.setSelection({
          col0: 0,
          row0: Math.min(a, row),
          col1: BIG,
          row1: Math.max(a, row),
        });
      }
      return;
    }

    if (dragging) {
      const cell = cellFromEvent(e);
      if (cell) props.extendSelection(cell.col, cell.row);
    }
  };

  const onMouseUp = () => {
    dragging = false;
    headerDrag = null;
    resizing = null;
  };

  const onDblClick = () => {
    if (props.store.editing) return;
    props.beginEdit();
  };

  const onContextMenu = (e: MouseEvent) => {
    e.preventDefault();
    const rect = canvas.getBoundingClientRect();
    const x = e.clientX - rect.left;
    const y = e.clientY - rect.top;
    const col = colAtCanvasX(x, props.scrollX(), props.widths());
    const row = rowAtCanvasY(y, props.scrollY(), props.heights());
    props.onContextMenu(e.clientX, e.clientY, col, row);
  };

  const onMouseHover = (e: MouseEvent) => {
    if (resizing || dragging || headerDrag) return;
    const rect = canvas.getBoundingClientRect();
    const x = e.clientX - rect.left;
    const y = e.clientY - rect.top;
    if (colResizeHit(x, props.scrollX(), props.widths()) >= 0) canvas.style.cursor = "col-resize";
    else if (rowResizeHit(y, props.scrollY(), props.heights()) >= 0) canvas.style.cursor = "row-resize";
    else if (y < HEADER_H || x < HEADER_W) canvas.style.cursor = "pointer";
    else canvas.style.cursor = "cell";
  };

  const onKeyDown = (e: KeyboardEvent) => {
    const a = props.store.active;
    if (props.store.editing) return; // editor handles its own keys
    const pageRows = Math.max(1, Math.floor((props.viewH() - HEADER_H) / DEFAULT_ROW_H));

    if (e.key === "Home") {
      e.preventDefault();
      if (e.ctrlKey) props.setActiveCell(0, 0);
      else props.setActiveCell(0, a.row);
      return;
    }
    if (e.key === "End") {
      e.preventDefault();
      if (e.ctrlKey) {
        let mc = a.col;
        let mr = a.row;
        for (const k of Object.keys(props.store.cells)) {
          const [c, r] = k.split(",").map(Number);
          if (c > mc) mc = c;
          if (r > mr) mr = r;
        }
        props.setActiveCell(mc, mr);
      } else {
        let c = a.col;
        while (hasData(c + 1, a.row)) c++;
        props.setActiveCell(c, a.row);
      }
      return;
    }
    if (e.key === "PageUp") {
      e.preventDefault();
      props.setActiveCell(a.col, Math.max(0, a.row - pageRows));
      return;
    }
    if (e.key === "PageDown") {
      e.preventDefault();
      props.setActiveCell(a.col, a.row + pageRows);
      return;
    }
    if (e.ctrlKey && (e.key === "ArrowUp" || e.key === "ArrowDown" || e.key === "ArrowLeft" || e.key === "ArrowRight")) {
      e.preventDefault();
      let c = a.col;
      let r = a.row;
      if (e.key === "ArrowUp") {
        while (r > 0 && hasData(c, r - 1)) r--;
      } else if (e.key === "ArrowDown") {
        while (hasData(c, r + 1)) r++;
      } else if (e.key === "ArrowLeft") {
        while (c > 0 && hasData(c - 1, r)) c--;
      } else {
        while (hasData(c + 1, r)) c++;
      }
      props.setActiveCell(c, r);
      return;
    }
    if (e.key === "ArrowUp") {
      props.setActiveCell(a.col, Math.max(0, a.row - 1));
      e.preventDefault();
    } else if (e.key === "ArrowDown") {
      props.setActiveCell(a.col, a.row + 1);
      e.preventDefault();
    } else if (e.key === "ArrowLeft") {
      props.setActiveCell(Math.max(0, a.col - 1), a.row);
      e.preventDefault();
    } else if (e.key === "ArrowRight") {
      props.setActiveCell(a.col + 1, a.row);
      e.preventDefault();
    } else if (e.key === "Enter" || e.key === "F2") {
      props.beginEdit();
      e.preventDefault();
    } else if (e.key === "Delete" || e.key === "Backspace") {
      props.commitEdit("");
      e.preventDefault();
    } else if (e.key.length === 1 && !e.ctrlKey && !e.metaKey && !e.altKey) {
      props.beginEdit(e.key);
      e.preventDefault();
    }
  };

  const editorPos = () => {
    const a = props.store.active;
    return {
      x: colX(a.col, props.widths()) - props.scrollX(),
      y: rowY(a.row, props.heights()) - props.scrollY(),
      w: colWidth(a.col, props.widths()),
      h: rowHeight(a.row, props.heights()),
    };
  };

  return (
    <div
      ref={container}
      style={{ position: "relative", flex: "1 1 auto", overflow: "hidden", outline: "none" }}
      tabindex={0}
      onWheel={onWheel}
      onMouseDown={onMouseDown}
      onMouseMove={(e) => {
        onMouseMove(e);
        onMouseHover(e);
      }}
      onMouseUp={onMouseUp}
      onDblClick={onDblClick}
      onContextMenu={onContextMenu}
      onKeyDown={onKeyDown}
    >
      <canvas ref={canvas} style={{ display: "block" }} />
      {props.store.editing && (
        <CellEditor
          pos={editorPos()}
          initial={props.editInitial()}
          onCommit={props.commitEdit}
          onCancel={props.cancelEdit}
        />
      )}
    </div>
  );
}
