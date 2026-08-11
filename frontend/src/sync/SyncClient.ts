import type { EngineClient } from "../engine/engineClient";
import type { Op } from "../types";
import { OpLog } from "./opQueue";

export interface SyncClientOptions {
  /** WebSocket URL of the sync server, e.g. `ws://127.0.0.1:8080/ws`. */
  url: string;
  engine: EngineClient;
  /** Called after a *remote* op is applied, so the UI can refresh. */
  onRemoteOp?: (op: Op) => void;
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

  constructor(private opts: SyncClientOptions) {
    this.actor = opts.actor ?? randomActor();
    if (opts.actor !== undefined) void opts.engine.init(this.actor);
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
    for (const op of this.pending) this.rawSend(op);
    this.pending = [];
  }

  private rawSend(op: Op) {
    if (this.ws && this.ws.readyState === WebSocket.OPEN) {
      this.ws.send(JSON.stringify(op));
    } else {
      this.pending.push(op);
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

  private onMessage(e: MessageEvent) {
    const op = JSON.parse(e.data as string) as Op;
    // Ignore our own ops echoed back by the server — they are already applied.
    if (opActor(op) === this.actor) return;
    void this.opts.engine.applyOps([op]);
    this.opts.onRemoteOp?.(op);
  }

  close() {
    this.closed = true;
    this.ws?.close();
  }
}
