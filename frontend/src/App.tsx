import { createEffect, createSignal, onMount } from "solid-js";
import { createStore } from "solid-js/store";
import { EngineClient } from "./engine/engineClient";
import { Grid } from "./grid/Grid";
import { AccessibleGrid, VISUALLY_HIDDEN } from "./grid/AccessibleGrid";
import { FormulaBar } from "./components/FormulaBar";
import { Toolbar } from "./components/Toolbar";
import { FormulaHelp } from "./components/FormulaHelp";
import { SheetTabs } from "./components/SheetTabs";
import { DependencyGraph } from "./components/DependencyGraph";
import { FindReplace } from "./components/FindReplace";
import { HistoryModal } from "./components/HistoryModal";
import { DiffModal } from "./components/DiffModal";
import { BranchModal } from "./components/BranchModal";
import { UdfModal } from "./components/UdfModal";
import { ContextMenu } from "./components/ContextMenu";
import { SyncClient } from "./sync/SyncClient";
import type { GridStore } from "./store";
import type { CellStyle, CellValue, Op } from "./types";
import { valueText } from "./types";
import { visibleRange, HEADER_W, HEADER_H, colX, rowY, colWidth, rowHeight, type Range } from "./grid/metrics";
import { toA1, parseA1, cellBitsToRC } from "./grid/coords";
import { adjustFormula } from "./grid/fill";

const BUFFER = 3;
const BIG = 1_000_000;

/** Captured state of a single cell, used for undo/redo. */
type CellState = { formula?: string; value?: CellValue };
type UndoEntry = { cells: Record<string, { before: CellState; after: CellState }> };

function parseInput(raw: string): CellValue {
  if (raw === "") return "Empty";
  if (raw.startsWith("=")) return "Empty"; // formulas handled via setFormula
  if (/^-?\d+(\.\d+)?$/.test(raw)) return { Number: parseFloat(raw) };
  if (raw === "true") return { Boolean: true };
  if (raw === "false") return { Boolean: false };
  return { Text: raw };
}

export function App() {
  const engine = new EngineClient();
  const [store, setStore] = createStore<GridStore>({
    cells: {},
    styles: {},
    active: { col: 0, row: 0 },
    selection: { col0: 0, row0: 0, col1: 0, row1: 0 },
    editing: false,
    rev: 0,
  });

  const [scrollX, setScrollX] = createSignal(0);
  const [scrollY, setScrollY] = createSignal(0);
  const [viewW, setViewW] = createSignal(800);
  const [viewH, setViewH] = createSignal(600);
  const [formulas, setFormulas] = createSignal<Record<string, string>>({});
  const [anchor, setAnchor] = createSignal({ col: 0, row: 0 });
  const [editInitial, setEditInitial] = createSignal("");
  const [colWidths, setColWidths] = createSignal<number[]>([]);
  const [rowHeights, setRowHeights] = createSignal<number[]>([]);
  const [undoStack, setUndoStack] = createSignal<UndoEntry[]>([]);
  const [redoStack, setRedoStack] = createSignal<UndoEntry[]>([]);
  const [remoteChanged, setRemoteChanged] = createSignal<Set<string>>(new Set());
  const [findMatches, setFindMatches] = createSignal<Set<string>>(new Set());
  const [findOpen, setFindOpen] = createSignal(false);
  const [contextMenu, setContextMenu] = createSignal<{ x: number; y: number; col: number; row: number } | null>(null);
  const [showHelp, setShowHelp] = createSignal(false);
  const [freezeCols, setFreezeCols] = createSignal(0);
  const [freezeRows, setFreezeRows] = createSignal(0);
  const [sheetNames, setSheetNames] = createSignal<string[]>(["Sheet1"]);
  const [activeSheet, setActiveSheet] = createSignal("Sheet1");
  const [showGraph, setShowGraph] = createSignal(false);
  const [showHistory, setShowHistory] = createSignal(false);
  const [showDiff, setShowDiff] = createSignal(false);
  const [showBranch, setShowBranch] = createSignal(false);
  const [showPlugins, setShowPlugins] = createSignal(false);
  // Client-only per-sheet state (the engine owns values + formulas content).
  type SheetCache = {
    widths: number[];
    heights: number[];
    styles: Record<string, CellStyle>;
    formulas: Record<string, string>;
  };
  const sheetCache = new Map<string, SheetCache>();

  let clipboardText = "";
  // Structured clipboard (positions + raw content) so a paste can preserve the
  // source cell of each formula and adjust its relative references (fill-style).
  type ClipCell = { col: number; row: number; value: string };
  let clipboard: ClipCell[] = [];
  let clipboardAnchor = { col: 0, row: 0 };

  // Remote presence cursors: actor id -> { cell, color }.
  const CURSOR_COLORS = [
    "#ef4444",
    "#f59e0b",
    "#10b981",
    "#3b82f6",
    "#8b5cf6",
    "#ec4899",
    "#14b8a6",
    "#f97316",
  ];
  const colorFor = (actor: number) =>
    CURSOR_COLORS[((actor % CURSOR_COLORS.length) + CURSOR_COLORS.length) % CURSOR_COLORS.length];
  const [remoteCursors, setRemoteCursors] = createSignal<Map<number, { cell: string; color: string }>>(
    new Map(),
  );
  let lastCursorCell = "";
  let cursorThrottle: number | undefined;
  const broadcastCursor = (cell: string) => {
    lastCursorCell = cell;
    if (cursorThrottle) return;
    cursorThrottle = window.setTimeout(() => {
      cursorThrottle = undefined;
      sync?.sendCursor(lastCursorCell);
    }, 60);
  };

  // --- accessibility: live-region announcements -----------------------------
  // Emit a polite announcement whenever the active cell or its value changes so
  // screen-reader users get the same feedback as the visual highlight.
  const [announcement, setAnnouncement] = createSignal("");
  createEffect(() => {
    const a = store.active;
    const a1 = toA1(a.col, a.row);
    const k = `${a.col},${a.row}`;
    const f = formulas()[k];
    const raw = f !== undefined ? f : valueText(store.cells[k] ?? "Empty");
    const suffix = f !== undefined ? " formula" : "";
    setAnnouncement(`${a1}${suffix}: ${raw === "" ? "(empty)" : raw}`);
  });

  // Sync client (created once the engine worker is ready). Non-reactive on purpose.
  let sync: SyncClient | null = null;
  const SYNC_URL = "ws://127.0.0.1:8080/ws";

  const key = (col: number, row: number) => `${col},${row}`;

  const bumpRev = () => setStore("rev", (r) => r + 1);

  const updateCell = (col: number, row: number, value: CellValue) => {
    setStore("cells", key(col, row), value);
    bumpRev();
  };

  const activeA1 = () => toA1(store.active.col, store.active.row);

  const activeRaw = () => {
    const k = key(store.active.col, store.active.row);
    const f = formulas()[k];
    if (f !== undefined) return f;
    return valueText(store.cells[k] ?? "Empty");
  };

  const activeStyle = (): CellStyle => store.styles[key(store.active.col, store.active.row)] ?? {};

  const clearRemote = (k: string) => {
    if (remoteChanged().has(k)) {
      const n = new Set<string>(remoteChanged());
      n.delete(k);
      setRemoteChanged(n);
    }
  };

  const ensureVisible = (col: number, row: number) => {
    const frozenCol = col < freezeCols();
    const frozenRow = row < freezeRows();
    const x = colX(col, colWidths());
    const y = rowY(row, rowHeights());
    const w = colWidth(col, colWidths());
    const h = rowHeight(row, rowHeights());
    // The scrollable viewport starts after the frozen strip (+ header gutters).
    const visLeft = HEADER_W + (freezeCols() > 0 ? colX(freezeCols(), colWidths()) - HEADER_W : 0);
    const visTop = HEADER_H + (freezeRows() > 0 ? rowY(freezeRows(), rowHeights()) - HEADER_H : 0);
    let sx = scrollX();
    let sy = scrollY();
    if (!frozenCol) {
      const cx = x - sx;
      if (cx < visLeft) sx = Math.max(0, x - visLeft);
      if (cx + w > sx + viewW()) sx = x + w - viewW();
    }
    if (!frozenRow) {
      const cy = y - sy;
      if (cy < visTop) sy = Math.max(0, y - visTop);
      if (cy + h > sy + viewH()) sy = y + h - viewH();
    }
    setScrollX(Math.max(0, sx));
    setScrollY(Math.max(0, sy));
  };

  const setActiveCell = (col: number, row: number) => {
    setAnchor({ col, row });
    setStore("active", { col, row });
    setStore("selection", { col0: col, row0: row, col1: col, row1: row });
    ensureVisible(col, row);
    broadcastCursor(toA1(col, row));
  };

  const setSelection = (range: Range) => {
    setStore("selection", range);
    setAnchor({ col: range.col0, row: range.row0 });
  };

  const extendSelection = (col: number, row: number) => {
    const a = anchor();
    setStore("selection", {
      col0: Math.min(a.col, col),
      row0: Math.min(a.row, row),
      col1: Math.max(a.col, col),
      row1: Math.max(a.row, row),
    });
  };

  const refreshVisible = async () => {
    const range = visibleRange(scrollX(), scrollY(), viewW(), viewH(), BUFFER, colWidths(), rowHeights());
    // Frozen leading rows/columns are always on screen, so always load them.
    if (freezeCols() > 0) range.col0 = 0;
    if (freezeRows() > 0) range.row0 = 0;
    const reads: Promise<void>[] = [];
    for (let r = range.row0; r <= range.row1; r++) {
      for (let c = range.col0; c <= range.col1; c++) {
        const a1 = toA1(c, r);
        reads.push(engine.getCell(a1).then((v) => updateCell(c, r, v)));
      }
    }
    await Promise.all(reads);
  };

  const evaluateAndRefresh = async () => {
    await engine.evaluate();
    await refreshVisible();
  };

  // --- mutation + undo/redo ------------------------------------------------

  const captureState = (a1: string): CellState => {
    const p = parseA1(a1);
    if (!p) return {};
    const k = key(p.col, p.row);
    const f = formulas()[k];
    if (f !== undefined) return { formula: f };
    const v = store.cells[k];
    if (v === undefined || v === "Empty") return {};
    return { value: v };
  };

  const applyCellState = async (a1: string, st: CellState) => {
    const p = parseA1(a1);
    if (!p) return;
    const k = key(p.col, p.row);
    clearRemote(k);
    if (st.formula !== undefined) {
      await engine.setFormula(a1, st.formula);
      setFormulas((f) => ({ ...f, [k]: st.formula! }));
    } else if (st.value !== undefined) {
      await engine.setCell(a1, st.value);
      setFormulas((f) => {
        const n = { ...f };
        delete n[k];
        return n;
      });
      updateCell(p.col, p.row, st.value);
    } else {
      await engine.deleteCell(a1);
      setFormulas((f) => {
        const n = { ...f };
        delete n[k];
        return n;
      });
      updateCell(p.col, p.row, "Empty");
    }
  };

  const pushUndo = (entry: UndoEntry) => {
    setUndoStack((s) => [...s, entry]);
    setRedoStack([]);
  };

  const mutate = async (entries: { a1: string; st: CellState }[]) => {
    if (!entries.length) return;
    const cells: Record<string, { before: CellState; after: CellState }> = {};
    for (const { a1, st } of entries) cells[a1] = { before: captureState(a1), after: st };
    for (const { a1, st } of entries) await applyCellState(a1, st);
    await evaluateAndRefresh();
    pushUndo({ cells });
  };

  const applyEntry = async (entry: UndoEntry, dir: "undo" | "redo") => {
    for (const a1 of Object.keys(entry.cells)) {
      const st = dir === "undo" ? entry.cells[a1].before : entry.cells[a1].after;
      await applyCellState(a1, st);
    }
    await evaluateAndRefresh();
  };

  const undo = async () => {
    const stack = undoStack();
    if (!stack.length) return;
    const entry = stack[stack.length - 1];
    setUndoStack(stack.slice(0, -1));
    setRedoStack((r) => [...r, entry]);
    await applyEntry(entry, "undo");
    void sync?.publishLocal();
  };

  const redo = async () => {
    const stack = redoStack();
    if (!stack.length) return;
    const entry = stack[stack.length - 1];
    setRedoStack(stack.slice(0, -1));
    setUndoStack((u) => [...u, entry]);
    await applyEntry(entry, "redo");
    void sync?.publishLocal();
  };

  const commitEdit = async (text: string) => {
    setStore("editing", false);
    const a1 = activeA1();
    let st: CellState;
    if (text === "") st = {};
    else if (text.startsWith("=")) st = { formula: text };
    else st = { value: parseInput(text) };
    await mutate([{ a1, st }]);
    void sync?.publishLocal();
  };

  const beginEdit = (initial?: string) => {
    setEditInitial(initial !== undefined ? initial : activeRaw());
    setStore("editing", true);
  };

  const cancelEdit = () => setStore("editing", false);

  // --- copy / paste --------------------------------------------------------

  const clampSel = (sel: Range): Range => {
    let maxR = sel.row0;
    let maxC = sel.col0;
    const scan = (k: string) => {
      const [c, r] = k.split(",").map(Number);
      if (c >= sel.col0 && c <= sel.col1 && r >= sel.row0 && r <= sel.row1) {
        if (c > maxC) maxC = c;
        if (r > maxR) maxR = r;
      }
    };
    Object.keys(store.cells).forEach(scan);
    Object.keys(formulas()).forEach(scan);
    return {
      col0: sel.col0,
      row0: sel.row0,
      col1: sel.col1 >= BIG ? maxC : sel.col1,
      row1: sel.row1 >= BIG ? maxR : sel.row1,
    };
  };

  const copySelection = () => {
    const sel = clampSel(store.selection);
    const rows: string[] = [];
    const cells: ClipCell[] = [];
    for (let r = sel.row0; r <= sel.row1; r++) {
      const out: string[] = [];
      for (let c = sel.col0; c <= sel.col1; c++) {
        const k = key(c, r);
        const f = formulas()[k];
        const v = f !== undefined ? f : valueText(store.cells[k] ?? "Empty");
        out.push(v);
        cells.push({ col: c, row: r, value: v });
      }
      rows.push(out.join("\t"));
    }
    const tsv = rows.join("\n");
    clipboardText = tsv;
    clipboard = cells;
    clipboardAnchor = { col: sel.col0, row: sel.row0 };
    void navigator.clipboard?.writeText(tsv).catch(() => {});
  };

  const paste = async () => {
    let text = clipboardText;
    try {
      const ext = await navigator.clipboard?.readText();
      if (ext) text = ext;
    } catch {
      /* fall back to in-app clipboard */
    }
    if (!text) return;
    const rows = text.split("\n").map((r) => r.split("\t"));
    const baseCol = store.active.col;
    const baseRow = store.active.row;
    const internal = clipboard.length > 0;
    const entries: { a1: string; st: CellState }[] = [];
    rows.forEach((rrow, ri) =>
      rrow.forEach((val, ci) => {
        if (val === "") return;
        const c = baseCol + ci;
        const r = baseRow + ri;
        const a1 = toA1(c, r);
        let st: CellState;
        if (val.startsWith("=")) {
          let f = val;
          if (internal) {
            const srcCol = clipboardAnchor.col + ci;
            const srcRow = clipboardAnchor.row + ri;
            f = adjustFormula(val, c - srcCol, r - srcRow);
          }
          st = { formula: f };
        } else {
          st = { value: parseInput(val) };
        }
        entries.push({ a1, st });
      }),
    );
    await mutate(entries);
    void sync?.publishLocal();
  };

  const clearSelection = async () => {
    const sel = clampSel(store.selection);
    const entries: { a1: string; st: CellState }[] = [];
    for (let r = sel.row0; r <= sel.row1; r++)
      for (let c = sel.col0; c <= sel.col1; c++) entries.push({ a1: toA1(c, r), st: {} });
    await mutate(entries);
    void sync?.publishLocal();
  };

  // Load a grid from a `.json` file in the LES grid format:
  // `{ "cells": { "A1": <value>, ... }, "formulas": { "B1": "=A1*2", ... } }`.
  // Replaces the current sheet contents.
  const openFile = async (file: File) => {
    const text = await file.text();
    let data: { cells?: Record<string, CellValue>; formulas?: Record<string, string> };
    try {
      data = JSON.parse(text);
    } catch {
      return;
    }
    await engine.reset();
    setStore("cells", {});
    setFormulas({});
    for (const [a1, value] of Object.entries(data.cells ?? {})) {
      if (parseA1(a1) && value) {
        await engine.setCell(a1, value);
        setStore("cells", a1, value);
      }
    }
    const formulaMap: Record<string, string> = {};
    for (const [a1, formula] of Object.entries(data.formulas ?? {})) {
      if (parseA1(a1) && formula) {
        await engine.setFormula(a1, formula);
        formulaMap[a1] = formula;
        setStore("cells", a1, "Empty");
      }
    }
    setFormulas(formulaMap);
    await evaluateAndRefresh();
    void sync?.publishLocal();
  };

  // --- formatting ----------------------------------------------------------

  const applyStyle = (patch: CellStyle) => {
    const sel = clampSel(store.selection);
    for (let r = sel.row0; r <= sel.row1; r++) {
      for (let c = sel.col0; c <= sel.col1; c++) {
        const k = key(c, r);
        setStore("styles", k, { ...(store.styles[k] ?? {}), ...patch });
      }
    }
    bumpRev();
  };

  // --- freeze panes --------------------------------------------------------
  // Freeze the leading rows/columns up to (and excluding) the active cell.
  const freezeAtActive = () => {
    setFreezeCols(store.active.col);
    setFreezeRows(store.active.row);
  };
  const unfreeze = () => {
    setFreezeCols(0);
    setFreezeRows(0);
  };

  // --- multi-sheet ----------------------------------------------------------
  // The engine holds a separate evaluator/CRDT per sheet; the frontend caches
  // the client-only view state (geometry, styles, raw formulas) per sheet and
  // swaps it in when the active sheet changes.
  const switchSheet = async (name: string) => {
    if (name === activeSheet() || !sheetNames().includes(name)) return;
    // Save the outgoing sheet's client-only state.
    sheetCache.set(activeSheet(), {
      widths: colWidths(),
      heights: rowHeights(),
      styles: { ...store.styles },
      formulas: { ...formulas() },
    });
    setActiveSheet(name);
    await engine.selectSheet(name);
    const c = sheetCache.get(name) ?? { widths: [], heights: [], styles: {}, formulas: {} };
    setColWidths(c.widths);
    setRowHeights(c.heights);
    setStore("styles", c.styles);
    setFormulas(c.formulas);
    setStore("cells", {});
    bumpRev();
    await evaluateAndRefresh();
  };

  const addSheet = async () => {
    const name = `Sheet${sheetNames().length + 1}`;
    await engine.newSheet(name);
    setSheetNames([...sheetNames(), name]);
    await switchSheet(name);
  };

  const deleteSheet = async (name: string) => {
    if (sheetNames().length <= 1) return;
    await engine.deleteSheet(name);
    const remaining = sheetNames().filter((n) => n !== name);
    setSheetNames(remaining);
    sheetCache.delete(name);
    if (activeSheet() === name) await switchSheet(remaining[0]);
  };

  const renameSheet = async (from: string, to: string) => {
    const trimmed = to.trim();
    if (!trimmed || sheetNames().includes(trimmed)) return;
    await engine.renameSheet(from, trimmed);
    setSheetNames(sheetNames().map((n) => (n === from ? trimmed : n)));
    if (sheetCache.has(from)) {
      const v = sheetCache.get(from)!;
      sheetCache.delete(from);
      sheetCache.set(trimmed, v);
    }
    if (activeSheet() === from) setActiveSheet(trimmed);
  };

  /// Re-sync the sheet tab list from the engine (e.g. after a fork adds a sheet).
  const refreshSheets = async () => {
    const { sheets } = await engine.listSheets();
    setSheetNames(sheets);
    if (!sheets.includes(activeSheet())) {
      await switchSheet(sheets[0]);
    }
  };

  // --- dependency-graph visualizer -----------------------------------------

  const insertRowAt = async (row: number) => {
    const idx = row <= 0 ? null : row - 1;
    await engine.insertRow(idx);
    await evaluateAndRefresh();
    void sync?.publishLocal();
    setActiveCell(store.active.col, store.active.row + 1);
  };

  const insertColumnAt = async (col: number) => {
    const idx = col <= 0 ? null : col - 1;
    await engine.insertColumn(idx);
    await evaluateAndRefresh();
    void sync?.publishLocal();
    setActiveCell(store.active.col + 1, store.active.row);
  };

  const deleteRowAt = async (row: number) => {
    await engine.deleteRow(row);
    await evaluateAndRefresh();
    void sync?.publishLocal();
    setActiveCell(store.active.col, Math.max(0, row - 1));
  };

  const deleteColumnAt = async (col: number) => {
    await engine.deleteColumn(col);
    await evaluateAndRefresh();
    void sync?.publishLocal();
    setActiveCell(Math.max(0, col - 1), store.active.row);
  };

  // --- find / replace ------------------------------------------------------

  const doFind = (query: string) => {
    if (!query) {
      setFindMatches(new Set<string>());
      return;
    }
    const q = query.toLowerCase();
    const matches = new Set<string>();
    for (const k of Object.keys(store.cells)) {
      const v = store.cells[k];
      const txt = valueText(v).toLowerCase();
      if (txt.includes(q)) matches.add(k);
    }
    for (const k of Object.keys(formulas())) {
      if (formulas()[k].toLowerCase().includes(q)) matches.add(k);
    }
    setFindMatches(matches);
    const first = matches.values().next().value as string | undefined;
    if (first) {
      const [c, r] = first.split(",").map(Number);
      setActiveCell(c, r);
    }
  };

  const nextMatch = () => {
    const m = findMatches();
    if (!m.size) return;
    const list = [...m];
    const cur = key(store.active.col, store.active.row);
    const i = list.indexOf(cur);
    const next = list[(i + 1) % list.length];
    const [c, r] = next.split(",").map(Number);
    setActiveCell(c, r);
  };

  const replaceOne = async (query: string, replacement: string, all: boolean) => {
    if (!query) return;
    const q = query.toLowerCase();
    const targets = [...findMatches()].filter((k) => {
      const v = store.cells[k];
      return valueText(v).toLowerCase().includes(q) || (formulas()[k] ?? "").toLowerCase().includes(q);
    });
    if (!targets.length) return;
    const entries: { a1: string; st: CellState }[] = [];
    for (const k of all ? targets : [targets[0]]) {
      const [c, r] = k.split(",").map(Number);
      const f = formulas()[k];
      const cur = f !== undefined ? f : valueText(store.cells[k] ?? "Empty");
      const next = cur.split(query).join(replacement);
      const st: CellState = next.startsWith("=") ? { formula: next } : { value: parseInput(next) };
      entries.push({ a1: toA1(c, r), st });
    }
    await mutate(entries);
    void sync?.publishLocal();
    doFind(query);
  };

  // --- context menu --------------------------------------------------------

  const openContextMenu = (clientX: number, clientY: number, col: number, row: number) => {
    setContextMenu({ x: clientX, y: clientY, col, row });
  };

  const cm = () => contextMenu();
  const cmRow = () => (cm() && cm()!.row >= 0 ? cm()!.row : store.active.row);
  const cmCol = () => (cm() && cm()!.col >= 0 ? cm()!.col : store.active.col);

  // --- remote change tracking (conflict UI) --------------------------------

  const decodeOpCell = (op: Op): { col: number; row: number } | null => {
    if ("SetCell" in op) return cellBitsToRC(op.SetCell.cell);
    if ("DeleteCell" in op) return cellBitsToRC(op.DeleteCell.cell);
    return null;
  };

  // Debounced refresh for scroll and for remote op arrivals.
  let refreshTimer: number | undefined;
  const scheduleRefresh = () => {
    if (refreshTimer) clearTimeout(refreshTimer);
    refreshTimer = window.setTimeout(() => {
      void refreshVisible();
    }, 150);
  };

  let scrollTimer: number | undefined;
  createEffect(() => {
    void scrollX();
    void scrollY();
    if (scrollTimer) clearTimeout(scrollTimer);
    scrollTimer = window.setTimeout(() => {
      void refreshVisible();
    }, 150);
  });

  onMount(async () => {
    await engine.ready;
    sync = new SyncClient({
      url: SYNC_URL,
      engine,
      onRemoteOp: (op) => {
        const cell = decodeOpCell(op);
        if (cell) {
          const k = key(cell.col, cell.row);
          setRemoteChanged((s) => {
            const n = new Set<string>(s);
            n.add(k);
            return n;
          });
          window.setTimeout(() => clearRemote(k), 6000);
        }
        scheduleRefresh();
      },
      onRemoteCursor: (actor, cell) => {
        setRemoteCursors((prev) => {
          const n = new Map(prev);
          n.set(actor, { cell, color: colorFor(actor) });
          return n;
        });
      },
    });
    await evaluateAndRefresh();
  });

  const onRemoteChanged = remoteChanged;

  return (
    <div style={{ display: "flex", "flex-direction": "column", height: "100vh" }}>
      <Toolbar
        onEvaluate={() => void evaluateAndRefresh()}
        onReset={async () => {
          await engine.reset();
          setStore("cells", {});
          setFormulas({});
          bumpRev();
          await evaluateAndRefresh();
        }}
        onUndo={() => void undo()}
        onRedo={() => void redo()}
        onFind={() => setFindOpen(true)}
        onHelp={() => setShowHelp(true)}
        onOpen={(f) => void openFile(f)}
        onToggleBold={() => applyStyle({ bold: !activeStyle().bold })}
        onToggleItalic={() => applyStyle({ italic: !activeStyle().italic })}
        onNumFmt={(fmt) => applyStyle({ numFmt: fmt })}
        onAlign={(align) => applyStyle({ align })}
        activeStyle={activeStyle}
        onFreeze={freezeAtActive}
        onUnfreeze={unfreeze}
        frozen={freezeCols() > 0 || freezeRows() > 0}
        onGraph={() => setShowGraph(true)}
        onHistory={() => setShowHistory(true)}
        onDiff={() => setShowDiff(true)}
        onBranch={() => setShowBranch(true)}
        onPlugins={() => setShowPlugins(true)}
      />
      <FormulaBar a1={activeA1} value={activeRaw} onCommit={(t) => void commitEdit(t)} />
      <Grid
        store={store}
        setStore={setStore}
        bumpRev={bumpRev}
        scrollX={scrollX}
        scrollY={scrollY}
        setScrollX={setScrollX}
        setScrollY={setScrollY}
        viewW={viewW}
        viewH={viewH}
        setView={(w, h) => {
          setViewW(w);
          setViewH(h);
        }}
        beginEdit={beginEdit}
        commitEdit={(t) => void commitEdit(t)}
        cancelEdit={cancelEdit}
        setActiveCell={setActiveCell}
        setSelection={setSelection}
        extendSelection={extendSelection}
        editInitial={editInitial}
        widths={colWidths}
        heights={rowHeights}
        setColWidth={(col, w) => setColWidths((a) => {
          const n = [...a];
          n[col] = w;
          return n;
        })}
        setRowHeight={(row, h) => setRowHeights((a) => {
          const n = [...a];
          n[row] = h;
          return n;
        })}
        onContextMenu={openContextMenu}
        remote={onRemoteChanged}
        find={findMatches}
        cursors={remoteCursors}
        freezeCols={freezeCols}
        freezeRows={freezeRows}
      />
      <AccessibleGrid store={store} formulas={formulas} />
      <div role="status" aria-live="polite" style={VISUALLY_HIDDEN}>
        {announcement()}
      </div>
      <SheetTabs
        sheets={sheetNames}
        active={activeSheet}
        onSelect={(n) => void switchSheet(n)}
        onAdd={() => void addSheet()}
        onDelete={(n) => void deleteSheet(n)}
        onRename={(from, to) => void renameSheet(from, to)}
      />
      <FindReplace
        open={findOpen()}
        matches={() => findMatches().size}
        onClose={() => setFindOpen(false)}
        onFind={(q) => doFind(q)}
        onReplace={(q, rep) => void replaceOne(q, rep, false)}
        onReplaceAll={(q, rep) => void replaceOne(q, rep, true)}
        onNext={() => nextMatch()}
      />
      {cm() && (
        <ContextMenu
          x={cm()!.x}
          y={cm()!.y}
          col={cm()!.col}
          row={cm()!.row}
          onCopy={() => copySelection()}
          onPaste={() => void paste()}
          onClear={() => void clearSelection()}
          onInsertRow={() => void insertRowAt(cmRow())}
          onInsertColumn={() => void insertColumnAt(cmCol())}
          onDeleteRow={() => void deleteRowAt(cmRow())}
          onDeleteColumn={() => void deleteColumnAt(cmCol())}
          onClose={() => setContextMenu(null)}
        />
      )}
      {showHelp() && <FormulaHelp onClose={() => setShowHelp(false)} />}
      {showGraph() && <DependencyGraph engine={engine} onClose={() => setShowGraph(false)} />}
      {showHistory() && (
        <HistoryModal
          engine={engine}
          onClose={() => setShowHistory(false)}
          onRefresh={() => void evaluateAndRefresh()}
        />
      )}
      {showDiff() && (
        <DiffModal
          engine={engine}
          onClose={() => setShowDiff(false)}
          onRefresh={() => void evaluateAndRefresh()}
        />
      )}
      {showBranch() && (
        <BranchModal
          engine={engine}
          onClose={() => setShowBranch(false)}
          onRefresh={() => void evaluateAndRefresh()}
          onSheetsChanged={() => void refreshSheets()}
          onSwitchTo={(n) => void switchSheet(n)}
        />
      )}
      {showPlugins() && <UdfModal engine={engine} onClose={() => setShowPlugins(false)} />}
    </div>
  );
}
