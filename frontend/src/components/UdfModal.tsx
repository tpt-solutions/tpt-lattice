import { createSignal, For, Show, onMount } from "solid-js";
import type { EngineClient } from "../engine/engineClient";

export interface UdfModalProps {
  engine: EngineClient;
  onClose: () => void;
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
  width: "min(92vw, 560px)",
  "max-height": "85vh",
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

/** Read a File's bytes as a `number[]` for the engine. */
async function fileBytes(file: File): Promise<number[]> {
  const buf = await file.arrayBuffer();
  return Array.from(new Uint8Array(buf));
}

export function UdfModal(props: UdfModalProps) {
  const [name, setName] = createSignal("");
  const [udfs, setUdfs] = createSignal<string[]>([]);
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  const [info, setInfo] = createSignal<string | null>(null);

  const load = async () => {
    try {
      setUdfs(await props.engine.listUdfs());
    } catch (e) {
      setError(String(e));
    }
  };

  const register = async (file: File | undefined) => {
    if (!file) return;
    const n = name().trim();
    if (!n) {
      setError("Give the plugin a function name (how you'll call it in a formula).");
      return;
    }
    setBusy(true);
    setError(null);
    setInfo(null);
    try {
      const bytes = await fileBytes(file);
      await props.engine.registerUdf(n, bytes);
      setName("");
      await load();
      setInfo(`Loaded "${n}". Call it from a formula like =${n}(A1, B1).`);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const unregister = async (n: string) => {
    setBusy(true);
    try {
      await props.engine.unregisterUdf(n);
      await load();
    } finally {
      setBusy(false);
    }
  };

  onMount(load);

  return (
    <div onClick={props.onClose} style={overlay}>
      <div onClick={(e) => e.stopPropagation()} style={panel}>
        <div style={headerBar}>
          <strong>UDF plugins</strong>
          <button onClick={props.onClose} style={{ padding: "2px 10px", cursor: "pointer" }}>
            ×
          </button>
        </div>
        <div style={{ padding: "8px 12px", display: "flex", gap: "6px", "align-items": "center", "flex-wrap": "wrap" }}>
          <input
            placeholder="Function name (e.g. DISCOUNT)"
            value={name()}
            onInput={(e) => setName(e.currentTarget.value)}
            style={{ padding: "4px 6px", border: "1px solid #e5e7eb", "border-radius": "4px" }}
          />
          <label style={{ ...btn }}>
            Load .wasm
            <input
              type="file"
              accept=".wasm,application/wasm"
              style={{ display: "none" }}
              onChange={(e) => void register(e.currentTarget.files?.[0])}
            />
          </label>
          <button style={btn} onClick={() => void register(undefined)} disabled={busy() || !name()}>
            Register
          </button>
        </div>
        <Show when={info()}>
          <div style={{ color: "#065f46", padding: "0 12px 8px" }}>{info()}</div>
        </Show>
        <Show when={error()}>
          <div style={{ color: "#b91c1c", padding: "0 12px 8px" }}>{error()}</div>
        </Show>
        <div style={{ "flex": "1 1 auto", overflow: "auto", padding: "0 12px 12px" }}>
          <div style={{ color: "#6b7280", "font-size": "12px", "margin-bottom": "6px" }}>
            Plugins run in a sandbox (no imports) and must export:{" "}
            <code>alloc(i32)-&gt;i32</code>, <code>dealloc(i32,i32)</code>,{" "}
            <code>call(i32,i32)-&gt;f64</code>, and <code>memory</code>. Args are f64s written into
            plugin memory; the result is an f64.
          </div>
          <For each={udfs()}>
            {(n) => (
              <div
                style={{
                  display: "flex",
                  "justify-content": "space-between",
                  "align-items": "center",
                  padding: "6px 0",
                  "border-bottom": "1px solid #f3f4f6",
                }}
              >
                <code>{n}</code>
                <button style={btn} onClick={() => void unregister(n)} disabled={busy()}>
                  Unregister
                </button>
              </div>
            )}
          </For>
          <Show when={udfs().length === 0}>
            <div style={{ color: "#6b7280" }}>No plugins loaded.</div>
          </Show>
        </div>
      </div>
    </div>
  );
}
