import { createSignal, For, Show, onMount } from "solid-js";
import type { EngineClient } from "../engine/engineClient";
import type { DiffRow } from "../types";
import { valueText } from "../types";

export interface DiffModalProps {
  engine: EngineClient;
  onClose: () => void;
  /** Called after a merge so the grid reloads. */
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
  width: "min(92vw, 720px)",
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

const statusColor: Record<string, string> = {
  added: "#dcfce7",
  removed: "#fee2e2",
  changed: "#fef3c7",
  unchanged: "#fff",
};

function sideText(c: { value: unknown; formula: string | null } | null): string {
  if (!c) return "—";
  if (c.formula) return c.formula;
  return c.value == null ? "∅" : valueText(c.value as never);
}

export function DiffModal(props: DiffModalProps) {
  const [versions, setVersions] = createSignal<[number, string, string][]>([]);
  const [left, setLeft] = createSignal(0);
  const [right, setRight] = createSignal(1);
  const [rows, setRows] = createSignal<DiffRow[]>([]);
  const [label, setLabel] = createSignal("");
  const [loading, setLoading] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);

  const load = async () => {
    setLoading(true);
    setError(null);
    try {
      const v = await props.engine.listVersions();
      setVersions(v);
      if (v.length >= 2) {
        setLeft(0);
        setRight(v.length - 1);
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  };

  const saveCurrent = async () => {
    const l = label().trim();
    if (!l) return;
    setLoading(true);
    try {
      await props.engine.saveVersion(l);
      setLabel("");
      await load();
    } finally {
      setLoading(false);
    }
  };

  const runDiff = async () => {
    setLoading(true);
    setError(null);
    try {
      const r = await props.engine.diff(left(), right());
      // Only surface meaningful changes by default; unchanged rows collapse.
      setRows(r.filter((x) => x.status !== "unchanged"));
      if (r.length === 0) setError("No saved versions to compare.");
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  };

  onMount(load);

  return (
    <div onClick={props.onClose} style={overlay}>
      <div onClick={(e) => e.stopPropagation()} style={panel}>
        <div style={headerBar}>
          <strong>Diff versions</strong>
          <button onClick={props.onClose} style={{ padding: "2px 10px", cursor: "pointer" }}>
            ×
          </button>
        </div>
        <div style={{ padding: "8px 12px", display: "flex", gap: "6px", "align-items": "center", "flex-wrap": "wrap" }}>
          <input
            placeholder="Version label…"
            value={label()}
            onInput={(e) => setLabel(e.currentTarget.value)}
            style={{ padding: "4px 6px", border: "1px solid #e5e7eb", "border-radius": "4px" }}
          />
          <button style={btn} onClick={() => void saveCurrent()} disabled={loading()}>
            Save current
          </button>
          <span style={{ color: "#6b7280" }}>
            {loading() ? "Working…" : `${versions().length} version(s)`}
          </span>
        </div>
        <Show when={versions().length >= 2}>
          <div style={{ padding: "0 12px 8px", display: "flex", gap: "8px", "align-items": "center" }}>
            <span>Compare</span>
            <select value={String(left())} onChange={(e) => setLeft(Number(e.currentTarget.value))}>
              <For each={versions()}>{([i, l]) => <option value={String(i)}>#{i} {l}</option>}</For>
            </select>
            <span>→</span>
            <select value={String(right())} onChange={(e) => setRight(Number(e.currentTarget.value))}>
              <For each={versions()}>{([i, l]) => <option value={String(i)}>#{i} {l}</option>}</For>
            </select>
            <button style={btn} onClick={() => void runDiff()} disabled={loading()}>
              Diff
            </button>
          </div>
        </Show>
        <div style={{ "flex": "1 1 auto", overflow: "auto", padding: "0 12px 12px" }}>
          <Show when={error()}>
            <div style={{ color: "#b91c1c" }}>{error()}</div>
          </Show>
          <Show when={versions().length < 2 && !error()}>
            <div style={{ color: "#6b7280" }}>Save at least two versions to diff them.</div>
          </Show>
          <For each={rows()}>
            {(r) => (
              <div
                style={{
                  display: "grid",
                  "grid-template-columns": "48px 1fr 1fr",
                  gap: "6px",
                  padding: "4px 6px",
                  "border-bottom": "1px solid #f3f4f6",
                  background: statusColor[r.status] ?? "#fff",
                }}
              >
                <span style={{ "font-weight": "700" }}>{r.cell}</span>
                <span style={{ color: "#374151", "font-family": "monospace" }}>{sideText(r.left)}</span>
                <span style={{ color: "#374151", "font-family": "monospace" }}>{sideText(r.right)}</span>
              </div>
            )}
          </For>
        </div>
        <div style={{ padding: "6px 12px", "border-top": "1px solid #e5e7eb", color: "#6b7280", "font-size": "12px" }}>
          Left = before, Right = after. Green: added · Red: removed · Amber: changed.
        </div>
      </div>
    </div>
  );
}
