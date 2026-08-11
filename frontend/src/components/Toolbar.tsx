import { createSignal } from "solid-js";

export interface ToolbarProps {
  onEvaluate: () => void;
  onReset: () => void;
}

/**
 * Stub formatting controls (bold/italic/number-format/alignment) plus working
 * Evaluate/Reset actions. The formatting buttons are visual-only for now — they
 * do not yet affect evaluation.
 */
export function Toolbar(props: ToolbarProps) {
  const [bold, setBold] = createSignal(false);
  const [italic, setItalic] = createSignal(false);

  const btn = (active: boolean) => ({
    padding: "4px 10px",
    border: "1px solid #e5e7eb",
    "border-radius": "4px",
    cursor: "pointer",
    background: active ? "#dbeafe" : "#fff",
    "font-weight": active ? "700" : "400",
  });

  return (
    <div
      style={{
        display: "flex",
        gap: "6px",
        padding: "6px 8px",
        "border-bottom": "1px solid #e5e7eb",
        background: "#f9fafb",
        "align-items": "center",
      }}
    >
      <button style={btn(bold())} onClick={() => setBold((b) => !b)}>
        <b>B</b>
      </button>
      <button style={btn(italic())} onClick={() => setItalic((i) => !i)}>
        <i>I</i>
      </button>
      <button style={btn(false)}># ,0.00</button>
      <button style={btn(false)}>≡</button>
      <div style={{ flex: "1 1 auto" }} />
      <button style={btn(false)} onClick={props.onEvaluate}>
        Evaluate
      </button>
      <button style={btn(false)} onClick={props.onReset}>
        Reset
      </button>
    </div>
  );
}
