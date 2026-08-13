import { createSignal, For, Show, onMount } from "solid-js";
import type { EngineClient } from "../engine/engineClient";
import type { MergeConflict } from "../types";
import { valueText } from "../types";

export interface BranchModalProps {
  engine: EngineClient;
  onClose: () => void;
  /** Reload the grid after a fork/merge. */
  onRefresh: () => void;
  /** Re-sync the sheet tab list (a fork adds a branch sheet). */
  onSheetsChanged: () => void;
  /** Switch the visible sheet (after merge/resolve we land on the parent). */
  onSwitchTo: (name: string) => void;
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
  width: "min(92vw, 640px)",
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

function sideText(c: { value: unknown; formula: string | null } | null): string {
  if (!c) return "∅";
  if (c.formula) return c.formula;
  return c.value == null ? "∅" : valueText(c.value as never);
}

export function BranchModal(props: BranchModalProps) {
  const [name, setName] = createSignal("");
  const [branches, setBranches] = createSignal<[string, string][]>([]);
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  const [result, setResult] = createSignal<string | null>(null);
  const [conflicts, setConflicts] = createSignal<MergeConflict[]>([]);
  const [lastBranch, setLastBranch] = createSignal("");

  const load = async () => {
    try {
      setBranches(await props.engine.listBranches());
    } catch (e) {
      setError(String(e));
    }
  };

  const fork = async () => {
    const n = name().trim();
    if (!n) return;
    setBusy(true);
    setError(null);
    setResult(null);
    try {
      await props.engine.fork(n);
      setName("");
      await load();
      props.onSheetsChanged();
      await props.onRefresh();
      setResult(`Forked into "${n}". Experiment there, then merge back.`);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const merge = async (branch: string) => {
    setBusy(true);
    setError(null);
    setResult(null);
    setConflicts([]);
    try {
      const m = await props.engine.mergeBranch(branch);
      if (!m) {
        setError("Merge failed.");
        return;
      }
      const [, parent] = branches().find(([b]) => b === branch) ?? ["", ""];
      setLastBranch(branch);
      setResult(`Merged "${branch}": ${m.applied} cell(s) applied automatically.`);
      setConflicts(m.conflicts);
      await props.onRefresh();
      props.onSwitchTo(parent);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const resolve = async (branch: string, c: MergeConflict, side: "ours" | "theirs") => {
    const [, parent] = branches().find(([b]) => b === branch) ?? ["", ""];
    const chosen = side === "theirs" ? c.theirs : c.ours;
    setBusy(true);
    try {
      await props.engine.selectSheet(parent);
      if (chosen?.formula) {
        await props.engine.setFormula(c.cell, chosen.formula);
      } else if (chosen && chosen.value !== null && chosen.value !== undefined) {
        await props.engine.setCell(c.cell, chosen.value as never);
      } else {
        await props.engine.deleteCell(c.cell);
      }
      await props.engine.evaluate();
      await props.onRefresh();
      setConflicts((cs) => cs.filter((x) => x.cell !== c.cell));
    } finally {
      setBusy(false);
    }
  };

  onMount(load);

  return (
    <div onClick={props.onClose} style={overlay}>
      <div onClick={(e) => e.stopPropagation()} style={panel}>
        <div style={headerBar}>
          <strong>What-if branches</strong>
          <button onClick={props.onClose} style={{ padding: "2px 10px", cursor: "pointer" }}>
            ×
          </button>
        </div>
        <div style={{ padding: "8px 12px", display: "flex", gap: "6px", "align-items": "center" }}>
          <input
            placeholder="Branch name…"
            value={name()}
            onInput={(e) => setName(e.currentTarget.value)}
            style={{ padding: "4px 6px", border: "1px solid #e5e7eb", "border-radius": "4px" }}
          />
          <button style={btn} onClick={() => void fork()} disabled={busy()}>
            Fork active sheet
          </button>
        </div>
        <Show when={error()}>
          <div style={{ color: "#b91c1c", padding: "0 12px 8px" }}>{error()}</div>
        </Show>
        <Show when={result()}>
          <div style={{ color: "#065f46", padding: "0 12px 8px" }}>{result()}</div>
        </Show>
        <div style={{ "flex": "1 1 auto", overflow: "auto", padding: "0 12px 12px" }}>
          <Show when={branches().length === 0}>
            <div style={{ color: "#6b7280" }}>
              No branches yet. Fork the active sheet to experiment safely; merge back when done.
            </div>
          </Show>
          <For each={branches()}>
            {([b, parent]) => (
              <div style={{ padding: "6px 0", "border-bottom": "1px solid #f3f4f6" }}>
                <div style={{ display: "flex", "justify-content": "space-between", "align-items": "center" }}>
                  <span>
                    <code>{b}</code> <span style={{ color: "#6b7280" }}>(from {parent})</span>
                  </span>
                  <button style={btn} onClick={() => void merge(b)} disabled={busy()}>
                    Merge back
                  </button>
                </div>
              </div>
            )}
          </For>
          <Show when={conflicts().length > 0}>
            <div style={{ "margin-top": "10px" }}>
              <strong style={{ color: "#b45309" }}>Conflicts — pick a side for each:</strong>
              <For each={conflicts()}>
                {(c) => (
                  <div
                    style={{
                      display: "grid",
                      "grid-template-columns": "48px 1fr 1fr auto",
                      gap: "6px",
                      "align-items": "center",
                      padding: "4px 0",
                      "border-bottom": "1px solid #f3f4f6",
                    }}
                  >
                    <span style={{ "font-weight": "700" }}>{c.cell}</span>
                    <span style={{ "font-family": "monospace", "font-size": "12px" }}>{sideText(c.ours)}</span>
                    <span style={{ "font-family": "monospace", "font-size": "12px" }}>{sideText(c.theirs)}</span>
                    <span style={{ display: "flex", gap: "4px" }}>
                      <button style={btn} onClick={() => void resolve(lastBranch(), c, "ours")} disabled={busy()}>ours</button>
                      <button style={btn} onClick={() => void resolve(lastBranch(), c, "theirs")} disabled={busy()}>theirs</button>
                    </span>
                  </div>
                )}
              </For>
            </div>
          </Show>
        </div>
      </div>
    </div>
  );
}
