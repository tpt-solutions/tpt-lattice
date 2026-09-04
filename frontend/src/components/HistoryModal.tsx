import { createSignal, For, Show, onMount } from "solid-js";
import type { EngineClient } from "../engine/engineClient";

export interface HistoryModalProps {
  engine: EngineClient;
  onClose: () => void;
  /** Called after a restore so the grid reloads the restored sheet. */
  onRefresh: () => void;
}

const overlay: Record<string, string> = {
  position: "fixed",
  inset: "0",
  background: "rgba(0,0,0,0.35)",
  display: "flex",
  "align-items": "center",
  "justify-content": "center",
  "z-index": "50",
};

const panel: Record<string, string> = {
  background: "#fff",
  "border-radius": "8px",
  width: "min(90vw, 460px)",
  "max-height": "80vh",
  display: "flex",
  "flex-direction": "column",
  overflow: "hidden",
  "box-shadow": "0 10px 40px rgba(0,0,0,0.25)",
};

const headerBar: Record<string, string> = {
  display: "flex",
  "justify-content": "space-between",
  "align-items": "center",
  padding: "8px 12px",
  "border-bottom": "1px solid #e5e7eb",
};

const btn: Record<string, string> = {
  padding: "4px 10px",
  border: "1px solid #e5e7eb",
  "border-radius": "4px",
  cursor: "pointer",
  background: "#fff",
};

export function HistoryModal(props: HistoryModalProps) {
  const [entries, setEntries] = createSignal<[number, string][]>([]);
  const [loading, setLoading] = createSignal(false);
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);

  const load = async () => {
    setLoading(true);
    setError(null);
    try {
      const r = await props.engine.request({ type: "ListHistory" });
      setEntries(r.type === "History" ? r.entries : []);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  };

  const restore = async (index: number) => {
    setBusy(true);
    try {
      await props.engine.request({ type: "Restore", index });
      props.onRefresh();
      await load();
    } finally {
      setBusy(false);
    }
  };

  const checkpoint = async () => {
    setBusy(true);
    try {
      await props.engine.request({
        type: "Checkpoint",
        label: new Date().toLocaleTimeString(),
      });
      await load();
    } finally {
      setBusy(false);
    }
  };

  onMount(load);

  return (
    <div onClick={props.onClose} style={overlay}>
      <div
        onClick={(e) => e.stopPropagation()}
        style={panel}
      >
        <div style={headerBar}>
          <strong>Version history</strong>
          <button onClick={props.onClose} style={{ padding: "2px 10px", cursor: "pointer" }}>
            ×
          </button>
        </div>
        <div
          style={{
            display: "flex",
            gap: "6px",
            "align-items": "center",
            padding: "8px 12px",
          }}
        >
          <button style={btn} onClick={() => void checkpoint()} disabled={busy()}>
            Checkpoint now
          </button>
          <button style={btn} onClick={() => void load()} disabled={loading()}>
            Refresh
          </button>
          <span style={{ color: "#6b7280" }}>
            {loading() ? "Loading…" : `${entries().length} checkpoint(s)`}
          </span>
        </div>
        <div style={{ "flex": "1 1 auto", overflow: "auto", padding: "0 12px 12px" }}>
          <Show when={error()}>
            <div style={{ color: "#b91c1c" }}>{error()}</div>
          </Show>
          <Show when={!error() && entries().length === 0}>
            <div style={{ color: "#6b7280" }}>No history yet — make some edits.</div>
          </Show>
          <For each={entries()}>
            {(entry) => (
              <div
                style={{
                  display: "flex",
                  "justify-content": "space-between",
                  "align-items": "center",
                  padding: "6px 0",
                  "border-bottom": "1px solid #f3f4f6",
                }}
              >
                <span>
                  <code>#{entry[0]}</code> <span style={{ color: "#374151" }}>{entry[1]}</span>
                </span>
                <button style={btn} onClick={() => void restore(entry[0])} disabled={busy()}>
                  Restore
                </button>
              </div>
            )}
          </For>
        </div>
      </div>
    </div>
  );
}
