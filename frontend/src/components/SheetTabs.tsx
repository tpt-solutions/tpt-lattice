import { createSignal, For } from "solid-js";

/**
 * Sheet tab strip. Single-sheet stub: add / rename / delete are wired to local UI
 * state only; multi-sheet engine support is not yet implemented.
 */
export function SheetTabs() {
  const [sheets, setSheets] = createSignal<string[]>(["Sheet1"]);
  const [active, setActive] = createSignal(0);

  const add = () => {
    setSheets((s) => [...s, `Sheet${s.length + 1}`]);
    setActive(sheets().length); // newly added index
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
      <For each={sheets()}>
        {(name, i) => (
          <button
            onClick={() => setActive(i())}
            style={{
              padding: "3px 12px",
              border: "1px solid #e5e7eb",
              "border-radius": "4px 4px 0 0",
              cursor: "pointer",
              background: i() === active() ? "#fff" : "transparent",
              "border-bottom": i() === active() ? "1px solid #fff" : "1px solid #e5e7eb",
            }}
          >
            {name}
          </button>
        )}
      </For>
      <button onClick={add} style={{ padding: "3px 10px", cursor: "pointer" }} title="Add sheet">
        +
      </button>
    </div>
  );
}
