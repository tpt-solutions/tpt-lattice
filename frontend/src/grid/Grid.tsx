import { Accessor, createEffect, onCleanup, onMount } from "solid-js";
import type { SetStoreFunction } from "solid-js/store";
import type { GridStore } from "../store";
import { CellEditor } from "./CellEditor";
import { COL_W, HEADER_H, HEADER_W, ROW_H } from "./metrics";
import { drawGrid } from "./renderer";

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
  extendSelection: (col: number, row: number) => void;
  editInitial: Accessor<string>;
}

export function Grid(props: GridProps) {
  let container!: HTMLDivElement;
  let canvas!: HTMLCanvasElement;
  let dragging = false;

  const cellFromEvent = (e: MouseEvent) => {
    const rect = canvas.getBoundingClientRect();
    const x = e.clientX - rect.left;
    const y = e.clientY - rect.top;
    const col = Math.floor((x + props.scrollX() - HEADER_W) / COL_W);
    const row = Math.floor((y + props.scrollY() - HEADER_H) / ROW_H);
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
    drawGrid({
      ctx,
      width: w,
      height: h,
      dpr,
      scrollX: props.scrollX(),
      scrollY: props.scrollY(),
      range: props.store.selection, // ensure redraw on selection moves
      cells: new Map(Object.entries(props.store.cells)),
      active: props.store.active,
      selection: props.store.selection,
      editing: props.store.editing,
    });
  });

  const onWheel = (e: WheelEvent) => {
    e.preventDefault();
    props.setScrollX(Math.max(0, props.scrollX() + e.deltaX));
    props.setScrollY(Math.max(0, props.scrollY() + e.deltaY));
  };

  const onMouseDown = (e: MouseEvent) => {
    const cell = cellFromEvent(e);
    if (!cell) return;
    dragging = true;
    props.setActiveCell(cell.col, cell.row);
  };

  const onMouseMove = (e: MouseEvent) => {
    if (!dragging) return;
    const cell = cellFromEvent(e);
    if (cell) props.extendSelection(cell.col, cell.row);
  };

  const onMouseUp = () => {
    dragging = false;
  };

  const onDblClick = () => {
    props.beginEdit();
  };

  const onKeyDown = (e: KeyboardEvent) => {
    const a = props.store.active;
    if (props.store.editing) return; // editor handles its own keys
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
      x: HEADER_W + a.col * COL_W - props.scrollX(),
      y: HEADER_H + a.row * ROW_H - props.scrollY(),
      w: COL_W,
      h: ROW_H,
    };
  };

  return (
    <div
      ref={container}
      style={{ position: "relative", flex: "1 1 auto", overflow: "hidden", outline: "none" }}
      tabindex={0}
      onWheel={onWheel}
      onMouseDown={onMouseDown}
      onMouseMove={onMouseMove}
      onMouseUp={onMouseUp}
      onDblClick={onDblClick}
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
