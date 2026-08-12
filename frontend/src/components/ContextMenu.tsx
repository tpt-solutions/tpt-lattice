import { For } from "solid-js";

export interface ContextMenuProps {
  x: number;
  y: number;
  /** Target column, or -1 when invoked from a row header. */
  col: number;
  /** Target row, or -1 when invoked from a column header. */
  row: number;
  onCopy: () => void;
  onPaste: () => void;
  onClear: () => void;
  onInsertRow: () => void;
  onInsertColumn: () => void;
  onDeleteRow: () => void;
  onDeleteColumn: () => void;
  onClose: () => void;
}

interface Item {
  label: string;
  action: () => void;
  disabled?: boolean;
}

export function ContextMenu(props: ContextMenuProps) {
  const onHeader = props.col < 0 || props.row < 0;
  const items: Item[] = [
    { label: "Copy", action: props.onCopy },
    { label: "Paste", action: props.onPaste },
    { label: "Clear contents", action: props.onClear },
    { label: "Insert row above", action: props.onInsertRow, disabled: onHeader && props.row < 0 },
    { label: "Insert column left", action: props.onInsertColumn, disabled: onHeader && props.col < 0 },
    { label: "Delete row", action: props.onDeleteRow, disabled: onHeader && props.row < 0 },
    { label: "Delete column", action: props.onDeleteColumn, disabled: onHeader && props.col < 0 },
  ];

  return (
    <>
      <div
        style={{ position: "fixed", inset: "0", "z-index": "20" }}
        onClick={() => props.onClose()}
        onContextMenu={(e) => {
          e.preventDefault();
          props.onClose();
        }}
      />
      <div
        style={{
          position: "fixed",
          left: `${props.x}px`,
          top: `${props.y}px`,
          "z-index": "21",
          "min-width": "160px",
          background: "#fff",
          border: "1px solid #e5e7eb",
          "border-radius": "6px",
          "box-shadow": "0 4px 12px rgba(0,0,0,0.12)",
          padding: "4px",
          "font-size": "13px",
        }}
      >
        <For each={items}>
          {(it) => (
            <div
              onClick={() => {
                if (it.disabled) return;
                it.action();
                props.onClose();
              }}
              style={{
                padding: "6px 10px",
                "border-radius": "4px",
                cursor: it.disabled ? "default" : "pointer",
                color: it.disabled ? "#9ca3af" : "#111827",
                background: it.disabled ? "transparent" : undefined,
              }}
              onMouseEnter={(e) => {
                if (!it.disabled) e.currentTarget.style.background = "#f3f4f6";
              }}
              onMouseLeave={(e) => (e.currentTarget.style.background = "transparent")}
            >
              {it.label}
            </div>
          )}
        </For>
      </div>
    </>
  );
}
