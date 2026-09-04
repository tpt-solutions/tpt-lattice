import { For } from "solid-js";
import type { Accessor } from "solid-js";

export interface SheetTabsProps {
  sheets: Accessor<string[]>;
  active: Accessor<string>;
  onSelect: (name: string) => void;
  onAdd: () => void;
  onDelete: (name: string) => void;
  onRename: (from: string, to: string) => void;
}

/**
 * Sheet tab strip. Each tab maps to a real sheet in the engine (separate
 * evaluator + CRDT). Add / rename / delete / select are wired to the engine.
 */
export function SheetTabs(props: SheetTabsProps) {
  const rename = (from: string) => {
    const to = window.prompt(`Rename "${from}" to:`, from);
    if (to != null && to.trim() && to.trim() !== from) props.onRename(from, to.trim());
  };

  return (
    <div
      style={{
        display: "flex",
        gap: "4px",
        padding: "4px 8px",
        "border-top": "1px solid #e5e7eb",
        background: "#f9fafb",
        "align-items": "center",
      }}
    >
      <For each={props.sheets()}>
        {(name) => (
          <div style={{ display: "flex", "align-items": "center" }}>
            <button
              onClick={() => props.onSelect(name)}
              onDblClick={() => rename(name)}
              title="Switch sheet (double-click to rename)"
              style={{
                padding: "3px 12px",
                border: "1px solid #e5e7eb",
                "border-radius": "4px 4px 0 0",
                cursor: "pointer",
                background: name === props.active() ? "#fff" : "transparent",
                "border-bottom":
                  name === props.active() ? "1px solid #fff" : "1px solid #e5e7eb",
              }}
            >
              {name}
            </button>
            <button
              onClick={() => props.onDelete(name)}
              title="Delete sheet"
              style={{
                "margin-left": "-1px",
                padding: "3px 6px",
                border: "1px solid #e5e7eb",
                cursor: "pointer",
                background: "#fff",
                "border-bottom": name === props.active() ? "1px solid #fff" : "1px solid #e5e7eb",
              }}
            >
              ×
            </button>
          </div>
        )}
      </For>
      <button onClick={props.onAdd} style={{ padding: "3px 10px", cursor: "pointer" }} title="Add sheet">
        +
      </button>
    </div>
  );
}
