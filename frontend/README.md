# TPT Lattice — Frontend

A [SolidJS](https://www.solidjs.com/) + [Vite](https://vitejs.dev/) + TypeScript
spreadsheet UI for the TPT Lattice engine. All engine computation runs inside a
**Web Worker** that hosts the compiled `tpt-lattice-wasm` package, so the main
thread stays responsive.

## Layout

```
src/
  types.ts                 # TS mirror of the wasm Request/Response protocol
  store.ts                 # reactive grid store shape
  engine/
    engine.worker.ts       # Web Worker hosting the wasm LatticeEngine
    engineClient.ts        # promise-based client routing all calls to the worker
  grid/
    coords.ts              # A1 <-> (col,row) helpers
    metrics.ts             # fixed grid geometry + visible-range math
    renderer.ts            # virtualized canvas drawing (pure)
    Grid.tsx               # canvas, scroll, selection, keyboard, editing overlay
    CellEditor.tsx         # <textarea> snapped over the active cell
  components/
    Toolbar.tsx            # formatting stubs + Evaluate/Reset
    FormulaBar.tsx         # active-cell address + raw formula/value
    SheetTabs.tsx          # sheet tab strip (single-sheet stub)
  App.tsx                  # orchestration: worker, store, refresh, editing
  index.tsx                # entry point
```

## Prerequisites

The UI consumes the compiled wasm engine from
`crates/tpt-lattice-wasm/pkg`. Build it once (from the repo root) before
running the frontend:

```sh
# Install the wasm toolchain (one time)
cargo install wasm-pack

# Build the engine package
wasm-pack build crates/tpt-lattice-wasm --target web --out-dir pkg
```

## Develop

```sh
npm install
npm run dev        # Vite dev server with HMR
```

Open the printed URL. The grid supports:

- Click / drag to select cells; arrow keys to move; Shift+arrows to extend.
- Double-click or press `Enter` / `F2` (or just start typing) to edit.
- Enter / Tab commits, Escape cancels. A leading `=` is treated as a formula.
- Scroll wheel pans the virtualized canvas; only the visible viewport is drawn.
- The formula bar shows the active cell's formula (if any) or its value.
- **Evaluate** re-runs the engine; **Reset** clears the grid.
- **Open** loads a grid from a `.json` file (see
  [`../../examples/README.md`](../../examples/README.md)); **Help** opens the LES
  formula reference.

### Keyboard shortcuts

The grid is fully keyboard-operable:

| Key | Action |
|-----|--------|
| `Arrow keys` | Move the active cell |
| `Shift` + `Arrows` | Extend the selection |
| `Ctrl`/`Cmd` + `Arrow` | Jump to the edge of the data region |
| `Home` / `Ctrl`+`Home` | Jump to column start / cell A1 |
| `End` / `Ctrl`+`End` | Jump to last value in row / last populated cell |
| `PageUp` / `PageDown` | Move one viewport up / down |
| `Enter` / `F2` / type | Start editing the active cell |
| `Tab` | Commit edit and move right |
| `Escape` | Cancel edit |
| `Delete` / `Backspace` | Clear the active cell |
| `Ctrl`+`Z` / `Ctrl`+`Shift`+`Z` | Undo / redo |

### Accessibility

- The canvas grid is mirrored into a visually-hidden `role="grid"` DOM tree
  (`grid/AccessibleGrid.tsx`) so screen readers can navigate cells; the canvas
  container uses `aria-activedescendant` to announce the focused cell.
- A polite live region (`aria-live="polite"`) announces the active cell and its
  value/ formula on every change.
- Errors are shown with both a red background **and** a warning glyph, so they are
  not conveyed by color alone.

## Build

```sh
npm run build      # tsc --noEmit && vite build  ->  dist/
npm run preview    # serve the production build
```
