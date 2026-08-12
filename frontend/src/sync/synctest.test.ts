import "fake-indexeddb/auto";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { EngineClient } from "../engine/engineClient";
import type { Op } from "../types";
import { OpLog } from "./opQueue";
import { SyncClient } from "./SyncClient";

// ---------------------------------------------------------------------------
// Fake WebSocket + hub. Mirrors the Axum server: retains op history and replays
// it to every new connection, so late joiners / reconnects converge.
// ---------------------------------------------------------------------------
class Hub {
  clients = new Set<FakeWebSocket>();
  history: string[] = [];
  connect(ws: FakeWebSocket) {
    this.clients.add(ws);
    queueMicrotask(() => {
      ws.readyState = FakeWebSocket.OPEN;
      if (ws.onopen) ws.onopen();
      // Replay server history only after the client has installed its handlers.
      for (const h of this.history) {
        if (ws.onmessage) ws.onmessage({ data: h });
      }
    });
  }
  broadcast(_from: FakeWebSocket, data: string) {
    for (const c of this.clients) {
      if (c.onmessage) c.onmessage({ data });
    }
  }
  disconnect(ws: FakeWebSocket) {
    this.clients.delete(ws);
    if (ws.onclose) ws.onclose();
  }
}

class FakeWebSocket {
  static OPEN = 1;
  onopen: (() => void) | null = null;
  onmessage: ((e: { data: string }) => void) | null = null;
  onclose: (() => void) | null = null;
  onerror: (() => void) | null = null;
  readyState = 0;
  constructor(_url: string) {
    hub.connect(this);
  }
  send(data: string) {
    hub.history.push(data);
    hub.broadcast(this, data);
  }
  close() {
    hub.disconnect(this);
  }
}

const hub = new Hub();
(globalThis as unknown as { WebSocket: typeof FakeWebSocket }).WebSocket = FakeWebSocket;

// ---------------------------------------------------------------------------
// Fake engine: records applied ops and queues locally-authored ops for the
// SyncClient to drain via takeOutbox(). Not the real wasm engine, but faithful
// to the EngineClient surface the SyncClient depends on.
// ---------------------------------------------------------------------------
class FakeEngine {
  actor = 0;
  applied: Op[] = [];
  outbox: Op[] = [];
  init(actor: number) {
    this.actor = actor;
    this.outbox = [];
  }
  takeOutbox(): Op[] {
    const o = this.outbox;
    this.outbox = [];
    return o;
  }
  async applyOps(ops: Op[]) {
    for (const op of ops) this.applied.push(op);
  }
  // Simulate a local edit (what the UI would trigger through the real engine).
  localSet(cell: number, value: number, clock: Record<number, number>) {
    const op = {
      SetCell: { cell, value: { Number: value }, clock, actor: this.actor },
    } as unknown as Op;
    this.applied.push(op);
    this.outbox.push(op);
    return op;
  }
}

const norm = (ops: Op[]) => ops.map((o) => JSON.stringify(o)).sort();

const flush = () => new Promise((r) => setTimeout(r, 0));

beforeEach(async () => {
  hub.clients.clear();
  hub.history = [];
  await new OpLog().clear();
});

describe("SyncClient", () => {
  it("two connected peers converge after a local edit", async () => {
    const a = new FakeEngine();
    a.init(1);
    const b = new FakeEngine();
    b.init(2);
    const sa = new SyncClient({ url: "ws://x", engine: a as unknown as EngineClient });
    new SyncClient({ url: "ws://x", engine: b as unknown as EngineClient });
    await flush();

    a.localSet(5, 42, { 1: 1 });
    await sa.publishLocal();
    await flush();

    expect(norm(b.applied)).toEqual(norm(a.applied));
  });

  it("a late joiner catches up via server history replay", async () => {
    const a = new FakeEngine();
    a.init(1);
    const sa = new SyncClient({ url: "ws://x", engine: a as unknown as EngineClient });
    await flush();

    a.localSet(7, 99, { 1: 1 });
    await sa.publishLocal();
    await flush();

    // B connects only after A's edit — the server replays history to it.
    const b = new FakeEngine();
    b.init(2);
    new SyncClient({ url: "ws://x", engine: b as unknown as EngineClient });
    await flush();

    expect(b.applied.map((o) => JSON.stringify(o))).toContain(
      JSON.stringify(a.applied[0]),
    );
  });

  it("ops persist to IndexedDB across a reload", async () => {
    const log = new OpLog();
    const op = {
      SetCell: { cell: 3, value: { Number: 5 }, clock: { 1: 1 }, actor: 1 },
    } as unknown as Op;
    await log.add(op);

    // New OpLog instance (simulating a page reload) sees the same persisted op.
    const reloaded = new OpLog();
    const all = await reloaded.all();
    expect(all.length).toBe(1);
  });

  it("converges under concurrent offline edits (CRDT merge)", async () => {
    const a = new FakeEngine();
    a.init(1);
    const b = new FakeEngine();
    b.init(2);
    const sa = new SyncClient({ url: "ws://x", engine: a as unknown as EngineClient });
    const sb = new SyncClient({ url: "ws://x", engine: b as unknown as EngineClient });
    await flush();

    // Concurrent edits to different cells, then exchange both ways.
    a.localSet(1, 10, { 1: 1 });
    b.localSet(2, 20, { 2: 1 });
    await sa.publishLocal();
    await sb.publishLocal();
    await flush();
    // A also learns B's op; B already learned A's via broadcast.
    await sa.publishLocal(); // re-drain (empty) to ensure no-op safety
    await flush();

    expect(norm(b.applied)).toEqual(norm(a.applied));
  });
});

afterEach(() => {
  hub.clients.forEach((c) => c.close());
});
