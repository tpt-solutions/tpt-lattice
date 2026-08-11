// Web Worker that hosts the TPT Lattice engine.
//
// The engine runs entirely off the main thread; the UI talks to it by sending
// `Request` JSON objects and receiving `Response` JSON objects (see the protocol
// documented in the crate README).
import init, { LatticeEngine } from "../pkg/tpt_lattice_wasm.js";

let engine = null;

async function boot() {
  await init();
  engine = new LatticeEngine();
  self.postMessage({ type: "ready" });
}

self.onmessage = (e) => {
  const msg = e.data;
  if (!engine) {
    self.postMessage({ id: msg.id, ok: false, error: "engine not ready" });
    return;
  }
  try {
    const responseJson = engine.handle(JSON.stringify(msg));
    self.postMessage({ id: msg.id, ok: true, response: JSON.parse(responseJson) });
  } catch (err) {
    self.postMessage({ id: msg.id, ok: false, error: String(err) });
  }
};

boot();
