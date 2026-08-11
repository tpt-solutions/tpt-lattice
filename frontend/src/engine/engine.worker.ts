/// <reference lib="webworker" />
import init, { LatticeEngine } from "@engine/tpt_lattice_wasm.js";
import type { Request, Response } from "../types";

let engine: LatticeEngine | null = null;

async function boot() {
  await init();
  engine = new LatticeEngine();
  (self as DedicatedWorkerGlobalScope).postMessage({ kind: "ready" });
}

self.onmessage = (e: MessageEvent) => {
  const msg = e.data as { id: number; req: Request };
  if (!engine) {
    (self as DedicatedWorkerGlobalScope).postMessage({
      id: msg.id,
      ok: false,
      error: "engine not ready",
    });
    return;
  }
  try {
    const out = engine.handle(JSON.stringify(msg.req));
    const response = JSON.parse(out) as Response;
    (self as DedicatedWorkerGlobalScope).postMessage({
      id: msg.id,
      ok: true,
      response,
    });
  } catch (err) {
    (self as DedicatedWorkerGlobalScope).postMessage({
      id: msg.id,
      ok: false,
      error: String(err),
    });
  }
};

boot();
