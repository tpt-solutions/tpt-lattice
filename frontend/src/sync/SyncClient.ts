import type { EngineClient } from "../engine/engineClient";
import type { Op } from "../types";
import { OpLog } from "./opQueue";

export interface SyncClientOptions {
  /** WebSocket URL of the sync server, e.g. `ws://127.0.0.1:8080/ws`. */
  url: string;
  engine: EngineClient;
  /** Called after a *remote* op is applied, so the UI can refresh. */
  onRemoteOp?: (op: Op) => void;
  /** Called when a remote peer moves its cursor: `(actor, cell)`. */
  onRemoteCursor?: (actor: number, cell: string) => void;
  /** Replica actor id. If omitted, a random one is generated. */
  actor?: number;
}

function randomActor(): number {
  return Math.floor(Math.random() * 0xffffffff);
}

function opActor(op: Op): number {
  const variant = Object.values(op)[0] as { actor?: number };
  return variant?.actor ?? -1;
}

/**
 * Bridges the engine worker and the sync server.
 *
 * - Local edits are drained from the engine's outbox, persisted to IndexedDB,
 *   and broadcast to peers.
 * - Remote ops are applied to the engine (CRDT merge) and trigger a UI refresh.
 * - The locally-authored op log is replayed on every (re)connect, and the server
 *   replays its retained history, so divergent peers reconverge.
 */
export class SyncClient {
  readonly actor: number;
  private ws: WebSocket | null = null;
  private readonly log = new OpLog();
  private pending: Op[] = [];
  private reconnectTimer?: ReturnType<typeof setTimeout>;
  private closed = false;
  /** Presence id assigned by the server on connect (falls back to `actor`). */
  private presenceId?: number;
  private remoteCursorCbs: ((actor: number, cell: string) => void)[] = [];

  constructor(private opts: SyncClientOptions) {
    this.actor = opts.actor ?? randomActor();
    // Always assign the engine this replica's (random or supplied) actor id so
    // the CRDT's last-writer-wins tie-break is deterministic per peer. The
    // previous code only initialized the engine when an explicit actor was
    // passed, so sessions that relied on the auto-generated id silently defaulted
    // to the engine's hard-coded `actor=1`, breaking convergence.
    void opts.engine.init(this.actor);
    this.connect();
  }

  private connect() {
    const ws = new WebSocket(this.opts.url);
    this.ws = ws;
    ws.onopen = () => this.replay();
    ws.onmessage = (e: MessageEvent) => this.onMessage(e);
    ws.onclose = () => this.scheduleReconnect();
    ws.onerror = () => ws.close();
  }

  private scheduleReconnect() {
    if (this.closed || this.reconnectTimer !== undefined) return;
    this.reconnectTimer = setTimeout(() => {
      this.reconnectTimer = undefined;
      this.connect();
    }, 1000);
  }

  /** Replay the persisted local op log (and any unsent ops) to the server. */
  private async replay() {
    const ops = await this.log.all();
    for (const op of ops) this.rawSend(op);
    // Snapshot pending before draining: `rawSend` may re-queue an op (e.g. if the
    // socket is not yet OPEN), and iterating the live array while it mutates would
    // otherwise loop forever.
    const pending = this.pending;
    this.pending = [];
    for (const op of pending) this.rawSend(op);
  }

  private rawSend(msg: unknown) {
    if (this.ws && this.ws.readyState === WebSocket.OPEN) {
      this.ws.send(JSON.stringify(msg));
    } else {
      // Cursor/non-op messages are ephemeral; only real ops are re-queued.
      if (typeof msg === "object" && msg !== null && "type" in msg && (msg as any).type === "cursor") {
        return;
      }
      this.pending.push(msg as Op);
    }
  }

  /**
   * Call after a local edit. Drains the engine's outbox, persists each op, and
   * broadcasts it. Safe to call whether or not the socket is connected.
   */
  async publishLocal() {
    const ops = await this.opts.engine.takeOutbox();
    for (const op of ops) {
      await this.log.add(op);
      this.rawSend(op);
    }
  }

  /** Register a callback for remote presence-cursor updates. */
  onRemoteCursor(cb: (actor: number, cell: string) => void) {
    this.remoteCursorCbs.push(cb);
  }

  /** Broadcast the local user's active cell to peers. */
  sendCursor(cell: string) {
    const actor = this.presenceId ?? this.actor;
    this.rawSend({ type: "cursor", cell, actor });
  }

  private onMessage(e: MessageEvent) {
    let msg: unknown;
    try {
      msg = JSON.parse(e.data as string);
    } catch {
      return;
    }
    const m = msg as { type?: string; actor?: number; cell?: string; id?: number };
    if (m && m.type === "welcome" && typeof m.id === "number") {
      this.presenceId = m.id;
      return;
    }
    if (m && m.type === "cursor" && typeof m.actor === "number" && typeof m.cell === "string") {
      // Ignore our own echoed cursor (the server broadcasts to all peers).
      if (m.actor !== (this.presenceId ?? this.actor)) {
        for (const cb of this.remoteCursorCbs) cb(m.actor, m.cell);
      }
      return;
    }
    const op = msg as Op;
    // Ignore our own ops echoed back by the server — they are already applied.
    if (opActor(op) === this.actor) return;
    // A malformed remote op must not produce an unhandled promise rejection on
    // every connected client; swallow and log it instead.
    void this.opts.engine
      .applyOps([op])
      .catch((err) => console.warn("failed to apply remote op", err));
    this.opts.onRemoteOp?.(op);
  }

  close() {
    this.closed = true;
    this.ws?.close();
  }
}
