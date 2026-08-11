import { createEffect, createSignal, onMount } from "solid-js";
import { createStore } from "solid-js/store";
import { EngineClient } from "./engine/engineClient";
import { Grid } from "./grid/Grid";
import { FormulaBar } from "./components/FormulaBar";
import { Toolbar } from "./components/Toolbar";
import { SheetTabs } from "./components/SheetTabs";
import { SyncClient } from "./sync/SyncClient";
import type { GridStore } from "./store";
import type { CellValue } from "./types";
import { COL_W, HEADER_H, HEADER_W, ROW_H, visibleRange } from "./grid/metrics";
import { toA1 } from "./grid/coords";
import { valueText } from "./types";

const BUFFER = 3;

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

  const ensureVisible = (col: number, row: number) => {
    const x = HEADER_W + col * COL_W;
    const y = HEADER_H + row * ROW_H;
    let sx = scrollX();
    let sy = scrollY();
    if (x < sx + HEADER_W) sx = Math.max(0, x - HEADER_W);
    if (x + COL_W > sx + viewW()) sx = x + COL_W - viewW();
    if (y < sy + HEADER_H) sy = Math.max(0, y - HEADER_H);
    if (y + ROW_H > sy + viewH()) sy = y + ROW_H - viewH();
    setScrollX(Math.max(0, sx));
    setScrollY(Math.max(0, sy));
  };

  const setActiveCell = (col: number, row: number) => {
    setAnchor({ col, row });
    setStore("active", { col, row });
    setStore("selection", { col0: col, row0: row, col1: col, row1: row });
    ensureVisible(col, row);
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
    const range = visibleRange(scrollX(), scrollY(), viewW(), viewH(), BUFFER);
    const reads: Promise<void>[] = [];
    for (let r = range.row0; r <= range.row1; r++) {
      for (let c = range.col0; c <= range.col1; c++) {
        const a1 = toA1(c, r);
        reads.push(
          engine.getCell(a1).then((v) => updateCell(c, r, v)),
        );
      }
    }
    await Promise.all(reads);
  };

  const evaluateAndRefresh = async () => {
    await engine.evaluate();
    await refreshVisible();
  };

  const beginEdit = (initial?: string) => {
    setEditInitial(initial !== undefined ? initial : activeRaw());
    setStore("editing", true);
  };

  const commitEdit = async (text: string) => {
    const col = store.active.col;
    const row = store.active.row;
    const a1 = toA1(col, row);
    const k = key(col, row);
    setStore("editing", false);
    try {
      if (text === "") {
        await engine.deleteCell(a1);
        setFormulas((f) => {
          const n = { ...f };
          delete n[k];
          return n;
        });
        updateCell(col, row, "Empty");
      } else if (text.startsWith("=")) {
        await engine.setFormula(a1, text);
        setFormulas((f) => ({ ...f, [k]: text }));
      } else {
        const val = parseInput(text);
        await engine.setCell(a1, val);
        setFormulas((f) => {
          const n = { ...f };
          delete n[k];
          return n;
        });
      }
      await evaluateAndRefresh();
      // Broadcast the op(s) we just authored to peers.
      void sync?.publishLocal();
    } catch (err) {
      console.error("edit failed", err);
    }
  };

  const cancelEdit = () => setStore("editing", false);

  // Debounced refresh for scroll and for remote op arrivals.
  let refreshTimer: number | undefined;
  const scheduleRefresh = () => {
    if (refreshTimer) clearTimeout(refreshTimer);
    refreshTimer = window.setTimeout(() => {
      void refreshVisible();
    }, 150);
  };

  // Debounced refresh while scrolling.
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
      onRemoteOp: () => scheduleRefresh(),
    });
    await evaluateAndRefresh();
  });

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
        extendSelection={extendSelection}
        editInitial={editInitial}
      />
      <SheetTabs />
    </div>
  );
}
