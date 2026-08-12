import { createSignal, type Accessor } from "solid-js";
import type { CellStyle } from "../types";

export interface ToolbarProps {
  onEvaluate: () => void;
  onReset: () => void;
  onUndo: () => void;
  onRedo: () => void;
  onFind: () => void;
  onHelp: () => void;
  onOpen: (file: File) => void;
  onToggleBold: () => void;
  onToggleItalic: () => void;
  onNumFmt: (fmt: NonNullable<CellStyle["numFmt"]>) => void;
  onAlign: (align: NonNullable<CellStyle["align"]>) => void;
  /** The style applied to the active cell, for reflecting toggle state. */
  activeStyle: Accessor<CellStyle>;
}

const btn = (active: boolean): Record<string, string> => ({
  padding: "4px 10px",
  border: "1px solid #e5e7eb",
  "border-radius": "4px",
  cursor: "pointer",
  background: active ? "#dbeafe" : "#fff",
  "font-weight": active ? "700" : "400",
});

export function Toolbar(props: ToolbarProps) {
  const [fmt, setFmt] = createSignal<NonNullable<CellStyle["numFmt"]>>("general");
  const style = () => props.activeStyle();
  let fileInput!: HTMLInputElement;

  return (
    <div
      style={{
        display: "flex",
        gap: "6px",
        padding: "6px 8px",
        "border-bottom": "1px solid #e5e7eb",
        background: "#f9fafb",
        "align-items": "center",
        "flex-wrap": "wrap",
      }}
    >
      <button style={btn(false)} onClick={props.onUndo} title="Undo (Ctrl+Z)">
        ↶
      </button>
      <button style={btn(false)} onClick={props.onRedo} title="Redo (Ctrl+Shift+Z)">
        ↷
      </button>
      <span style={{ width: "1px", height: "20px", background: "#e5e7eb" }} />
      <button style={btn(!!style().bold)} onClick={props.onToggleBold} title="Bold">
        <b>B</b>
      </button>
      <button style={btn(!!style().italic)} onClick={props.onToggleItalic} title="Italic">
        <i>I</i>
      </button>
      <select
        value={fmt()}
        onChange={(e) => {
          const v = e.currentTarget.value as NonNullable<CellStyle["numFmt"]>;
          setFmt(v);
          props.onNumFmt(v);
        }}
        style={{ padding: "4px 6px", border: "1px solid #e5e7eb", "border-radius": "4px" }}
        title="Number format"
      >
        <option value="general">General</option>
        <option value="number">#,##0.00</option>
        <option value="percent">0%</option>
        <option value="currency">$#,##0.00</option>
      </select>
      <button style={btn(style().align === "left")} onClick={() => props.onAlign("left")} title="Align left">
        ≡
      </button>
      <button style={btn(style().align === "center")} onClick={() => props.onAlign("center")} title="Align center">
        ≣
      </button>
      <button style={btn(style().align === "right")} onClick={() => props.onAlign("right")} title="Align right">
        ⇥
      </button>
      <div style={{ flex: "1 1 auto" }} />
      <button style={btn(false)} onClick={props.onFind} title="Find / Replace">
        Find
      </button>
      <button style={btn(false)} onClick={props.onHelp} title="LES formula reference">
        Help
      </button>
      <button style={btn(false)} onClick={() => fileInput.click()} title="Open a .json grid">
        Open
      </button>
      <input
        ref={fileInput}
        type="file"
        accept="application/json,.json"
        style={{ display: "none" }}
        onChange={(e) => {
          const f = e.currentTarget.files?.[0];
          if (f) props.onOpen(f);
          e.currentTarget.value = "";
        }}
      />
      <button style={btn(false)} onClick={props.onEvaluate}>
        Evaluate
      </button>
      <button style={btn(false)} onClick={props.onReset}>
        Reset
      </button>
    </div>
  );
}
