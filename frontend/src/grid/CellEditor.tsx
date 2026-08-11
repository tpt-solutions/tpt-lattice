import { onMount } from "solid-js";

export interface CellEditorProps {
  pos: { x: number; y: number; w: number; h: number };
  initial?: string;
  onCommit: (text: string) => void;
  onCancel: () => void;
}

/**
 * Absolutely-positioned textarea snapped over the active cell. Commits on
 * Enter / Tab, cancels on Escape. The initial text is supplied by the parent
 * (the cell's formula or value).
 */
export function CellEditor(props: CellEditorProps) {
  let ref!: HTMLTextAreaElement;

  onMount(() => {
    ref.focus();
    ref.value = props.initial ?? "";
    ref.select();
  });

  const commit = () => props.onCommit(ref.value);
  const onKeyDown = (e: KeyboardEvent) => {
    if (e.key === "Enter") {
      e.preventDefault();
      commit();
    } else if (e.key === "Tab") {
      e.preventDefault();
      commit();
    } else if (e.key === "Escape") {
      e.preventDefault();
      props.onCancel();
    }
  };

  return (
    <textarea
      ref={ref}
      style={{
        position: "absolute",
        left: `${props.pos.x}px`,
        top: `${props.pos.y}px`,
        width: `${props.pos.w}px`,
        height: `${props.pos.h}px`,
        "box-sizing": "border-box",
        "font-family": "ui-monospace, monospace",
        "font-size": "13px",
        border: "2px solid #2563eb",
        padding: "0 4px",
        margin: "0",
        resize: "none",
        outline: "none",
        "z-index": "10",
      }}
      onKeyDown={onKeyDown}
      onBlur={commit}
    />
  );
}
