import { Accessor, createSignal } from "solid-js";

export interface FindReplaceProps {
  open: boolean;
  matches: Accessor<number>;
  onClose: () => void;
  onFind: (query: string) => void;
  onReplace: (query: string, replacement: string) => void;
  onReplaceAll: (query: string, replacement: string) => void;
  onNext: () => void;
}

export function FindReplace(props: FindReplaceProps) {
  const [query, setQuery] = createSignal("");
  const [replacement, setReplacement] = createSignal("");

  if (!props.open) return null;

  const rowStyle: Record<string, string> = {
    display: "flex",
    gap: "6px",
    "align-items": "center",
    padding: "4px 0",
  };

  return (
    <div
      style={{
        position: "fixed",
        right: "16px",
        top: "16px",
        width: "320px",
        background: "#fff",
        border: "1px solid #e5e7eb",
        "border-radius": "8px",
        "box-shadow": "0 6px 20px rgba(0,0,0,0.15)",
        padding: "12px",
        "z-index": "30",
        "font-size": "13px",
      }}
    >
      <div style={{ ...rowStyle }}>
        <input
          placeholder="Find"
          value={query()}
          onInput={(e) => setQuery(e.currentTarget.value)}
          style={{ flex: "1 1 auto", padding: "4px 8px", border: "1px solid #e5e7eb", "border-radius": "4px" }}
        />
        <button style={btn} onClick={() => props.onFind(query())}>
          Find
        </button>
      </div>
      <div style={{ ...rowStyle }}>
        <input
          placeholder="Replace"
          value={replacement()}
          onInput={(e) => setReplacement(e.currentTarget.value)}
          style={{ flex: "1 1 auto", padding: "4px 8px", border: "1px solid #e5e7eb", "border-radius": "4px" }}
        />
        <button style={btn} onClick={() => props.onReplace(query(), replacement())}>
          Replace
        </button>
        <button style={btn} onClick={() => props.onReplaceAll(query(), replacement())}>
          All
        </button>
      </div>
      <div style={{ ...rowStyle, "justify-content": "space-between" }}>
        <span style={{ color: "#6b7280" }}>{props.matches()} match(es)</span>
        <span>
          <button style={btn} onClick={() => props.onNext()}>
            Next
          </button>
          <button style={btn} onClick={() => props.onClose()}>
            Close
          </button>
        </span>
      </div>
    </div>
  );
}

const btn: Record<string, string> = {
  padding: "4px 10px",
  border: "1px solid #e5e7eb",
  "border-radius": "4px",
  cursor: "pointer",
  background: "#fff",
};
