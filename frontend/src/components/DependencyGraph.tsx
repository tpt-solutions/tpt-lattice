import { createMemo, createSignal, For, onMount } from "solid-js";
import type { EngineClient } from "../engine/engineClient";

export interface DependencyGraphProps {
  engine: EngineClient;
  onClose: () => void;
}

const NODE_W = 56;
const NODE_H = 24;
const COL_GAP = 44;
const ROW_GAP = 16;
const PAD = 24;

/**
 * Visualizes the active sheet's cell dependency DAG (the graph the evaluator
 * already maintains). Nodes are laid out in dependency-order columns: a cell's
 * column is the longest path from a root dependency, so arrows always point
 * left-to-right.
 */
export function DependencyGraph(props: DependencyGraphProps) {
  const [nodes, setNodes] = createSignal<{ id: string; x: number; y: number }[]>([]);
  const [edges, setEdges] = createSignal<{ from: string; to: string }[]>([]);
  const [error, setError] = createSignal<string | null>(null);

  const pos = createMemo(() => {
    const m = new Map<string, { x: number; y: number }>();
    for (const n of nodes()) m.set(n.id, { x: n.x, y: n.y });
    return m;
  });

  const width = () =>
    nodes().reduce((m, n) => Math.max(m, n.x + NODE_W), 0) + PAD;
  const height = () =>
    nodes().reduce((m, n) => Math.max(m, n.y + NODE_H), 0) + PAD;

  onMount(async () => {
    try {
      const g = await props.engine.getGraph();
      layout(g.nodes, g.edges);
    } catch (e) {
      setError(String(e));
    }
  });

  const layout = (rawNodes: string[], rawEdges: [string, string][]) => {
    const adj = new Map<string, string[]>();
    const indeg = new Map<string, number>();
    for (const n of rawNodes) {
      adj.set(n, []);
      indeg.set(n, 0);
    }
    for (const [dep, dependent] of rawEdges) {
      if (!adj.has(dep) || !adj.has(dependent)) continue;
      adj.get(dep)!.push(dependent);
      indeg.set(dependent, (indeg.get(dependent) ?? 0) + 1);
    }

    // Kahn topological order, assigning each node the longest path from a root.
    const level = new Map<string, number>();
    const q: string[] = [];
    for (const n of rawNodes) {
      if ((indeg.get(n) ?? 0) === 0) {
        level.set(n, 0);
        q.push(n);
      }
    }
    const order: string[] = [];
    while (q.length) {
      const n = q.shift()!;
      order.push(n);
      for (const m of adj.get(n) ?? []) {
        level.set(m, Math.max(level.get(m) ?? 0, (level.get(n) ?? 0) + 1));
        indeg.set(m, (indeg.get(m) ?? 0) - 1);
        if (indeg.get(m) === 0) q.push(m);
      }
    }
    for (const n of rawNodes) if (!level.has(n)) level.set(n, 0);

    const byLevel = new Map<number, string[]>();
    for (const n of order.length ? order : rawNodes) {
      const lv = level.get(n) ?? 0;
      if (!byLevel.has(lv)) byLevel.set(lv, []);
      byLevel.get(lv)!.push(n);
    }

    const list: { id: string; x: number; y: number }[] = [];
    for (const [lv, ns] of byLevel) {
      ns.forEach((n, i) => {
        list.push({
          id: n,
          x: PAD + lv * (NODE_W + COL_GAP),
          y: PAD + i * (NODE_H + ROW_GAP),
        });
      });
    }
    setNodes(list);
    setEdges(
      rawEdges
        .filter(([d, m]) => d !== m && list.some((n) => n.id === d) && list.some((n) => n.id === m))
        .map(([d, m]) => ({ from: d, to: m })),
    );
  };

  return (
    <div
      onClick={props.onClose}
      style={{
        position: "fixed",
        inset: "0",
        background: "rgba(0,0,0,0.35)",
        display: "flex",
        "align-items": "center",
        "justify-content": "center",
        "z-index": "50",
      }}
    >
      <div
        onClick={(e) => e.stopPropagation()}
        style={{
          background: "#fff",
          "border-radius": "8px",
          width: "min(90vw, 900px)",
          height: "min(80vh, 640px)",
          display: "flex",
          "flex-direction": "column",
          overflow: "hidden",
          "box-shadow": "0 10px 40px rgba(0,0,0,0.25)",
        }}
      >
        <div
          style={{
            display: "flex",
            "justify-content": "space-between",
            "align-items": "center",
            padding: "8px 12px",
            "border-bottom": "1px solid #e5e7eb",
          }}
        >
          <strong>Dependency graph — active sheet</strong>
          <button onClick={props.onClose} style={{ padding: "2px 10px", cursor: "pointer" }}>
            ×
          </button>
        </div>
        <div style={{ flex: "1 1 auto", overflow: "auto", padding: "8px" }}>
          {error() && <div style={{ color: "#b91c1c" }}>{error()}</div>}
          {!error() && nodes().length === 0 && (
            <div style={{ color: "#6b7280" }}>No formulas on this sheet yet.</div>
          )}
          <svg width={width()} height={height()}>
            <For each={edges()}>
              {(e) => {
                const a = pos().get(e.from);
                const b = pos().get(e.to);
                if (!a || !b) return null;
                const x1 = a.x + NODE_W;
                const y1 = a.y + NODE_H / 2;
                const x2 = b.x;
                const y2 = b.y + NODE_H / 2;
                return (
                  <line
                    x1={x1}
                    y1={y1}
                    x2={x2}
                    y2={y2}
                    stroke="#9ca3af"
                    stroke-width="1.5"
                    marker-end="url(#arrow)"
                  />
                );
              }}
            </For>
            <defs>
              <marker id="arrow" markerWidth="8" markerHeight="8" refX="6" refY="3" orient="auto">
                <path d="M0,0 L6,3 L0,6 Z" fill="#9ca3af" />
              </marker>
            </defs>
            <For each={nodes()}>
              {(n) => (
                <g>
                  <rect
                    x={n.x}
                    y={n.y}
                    width={NODE_W}
                    height={NODE_H}
                    rx="4"
                    fill="#eff6ff"
                    stroke="#2563eb"
                  />
                  <text
                    x={n.x + NODE_W / 2}
                    y={n.y + NODE_H / 2}
                    text-anchor="middle"
                    dominant-baseline="middle"
                    font-size="11"
                    font-family="ui-monospace, monospace"
                    fill="#1e3a8a"
                  >
                    {n.id}
                  </text>
                </g>
              )}
            </For>
          </svg>
        </div>
      </div>
    </div>
  );
}
