import { Accessor, createEffect, createSignal } from "solid-js";

export interface FormulaBarProps {
  a1: Accessor<string>;
  /** Raw text for the active cell: its formula if present, else its value. */
  value: Accessor<string>;
  onCommit: (text: string) => void;
}

export function FormulaBar(props: FormulaBarProps) {
  const [text, setText] = createSignal(props.value());
  let focused = false;

  createEffect(() => {
    if (!focused) setText(props.value());
  });

  return (
    <div
      style={{
        display: "flex",
        gap: "8px",
        padding: "6px 8px",
        "border-bottom": "1px solid #e5e7eb",
        background: "#fff",
        "align-items": "center",
      }}
    >
      <div
        style={{
          "min-width": "64px",
          "text-align": "center",
          "font-family": "ui-monospace, monospace",
          color: "#374151",
          border: "1px solid #e5e7eb",
          "border-radius": "4px",
          padding: "2px 6px",
        }}
      >
        {props.a1()}
      </div>
      <input
        style={{
          flex: "1 1 auto",
          "font-family": "ui-monospace, monospace",
          padding: "4px 8px",
          border: "1px solid #e5e7eb",
          "border-radius": "4px",
        }}
        value={text()}
        onFocus={() => {
          focused = true;
          setText(props.value());
        }}
        onBlur={() => {
          focused = false;
        }}
        onInput={(e) => setText(e.currentTarget.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") props.onCommit(text());
        }}
      />
    </div>
  );
}
