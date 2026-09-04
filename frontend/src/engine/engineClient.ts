import type { CellValue, DiffRow, MergeConflict, Op, Request, Response } from "../types";

type Pending = (r: { ok: boolean; response?: Response; error?: string }) => void;

/**
 * Promise-based wrapper around the engine Web Worker. Every engine call the UI
 * makes is routed through this client, so the main thread never blocks on the
 * wasm computation. The worker owns a single `LatticeEngine` instance.
 */
export class EngineClient {
  private worker: Worker;
  private seq = 0;
  private pending = new Map<number, Pending>();
  private resolveReady?: () => void;
  readonly ready: Promise<void>;

  constructor() {
    this.worker = new Worker(new URL("./engine.worker.ts", import.meta.url), {
      type: "module",
    });
    this.ready = new Promise<void>((resolve) => {
      this.resolveReady = resolve;
    });
    this.worker.addEventListener("message", this.onMessage);
  }

  private onMessage = (e: MessageEvent) => {
    const data = e.data;
    if (data && data.kind === "ready") {
      this.resolveReady?.();
      this.resolveReady = undefined;
      return;
    }
    const { id, ok, response, error } = data;
    const p = this.pending.get(id);
    if (p) {
      this.pending.delete(id);
      p({ ok, response, error });
    }
  };

  private send(req: Request): Promise<Response> {
    const id = ++this.seq;
    return new Promise<Response>((resolve, reject) => {
      this.pending.set(id, (r) => {
        if (r.ok && r.response) resolve(r.response);
        else reject(new Error(r.error || "engine error"));
      });
      this.worker.postMessage({ id, req });
    });
  }

  setCell(cell: string, value: CellValue) {
    return this.send({ type: "SetCell", cell, value });
  }

  setFormula(cell: string, formula: string) {
    return this.send({ type: "SetFormula", cell, formula });
  }

  async   getCell(cell: string): Promise<CellValue> {
    const r = await this.send({ type: "GetCell", cell });
    return r.type === "Value" ? r.value : "Empty";
  }

  /** Assign this replica a unique actor id (call once before editing). */
  init(actor: number) {
    return this.send({ type: "Init", actor });
  }

  /** Delete a cell, recording an op for sync. */
  deleteCell(cell: string) {
    return this.send({ type: "DeleteCell", cell });
  }

  /** Drain and return ops this replica has authored since the last call. */
  async takeOutbox(): Promise<Op[]> {
    const r = await this.send({ type: "TakeOutbox" });
    return r.type === "Outbox" ? r.ops : [];
  }

  /** Low-level passthrough for any `Request` (used by newer engine features). */
  request(req: Request): Promise<Response> {
    return this.send(req);
  }

  /** Return every materialized `(A1, value)` pair (for find/replace). */
  async listCells(): Promise<{ cell: string; value: CellValue }[]> {
    const r = await this.send({ type: "ListCells" });
    return r.type === "Cells" ? r.cells : [];
  }

  /** Create a new (empty) sheet. */
  newSheet(name: string) {
    return this.send({ type: "NewSheet", name });
  }

  /** Delete a sheet by name (refused when it is the last sheet). */
  deleteSheet(name: string) {
    return this.send({ type: "DeleteSheet", name });
  }

  /** Rename a sheet (`from` -> `to`). */
  renameSheet(from: string, to: string) {
    return this.send({ type: "RenameSheet", from, to });
  }

  /** Make `name` the active sheet for subsequent requests. */
  selectSheet(name: string) {
    return this.send({ type: "SelectSheet", name });
  }

  /** List sheet names and the active sheet. */
  async listSheets(): Promise<{ sheets: string[]; active: string }> {
    const r = await this.send({ type: "ListSheets" });
    return r.type === "Sheets" ? { sheets: r.sheets, active: r.active } : { sheets: [], active: "" };
  }

  /** Return the active sheet's dependency graph (DAG) as A1 nodes + edges. */
  async getGraph(): Promise<{ nodes: string[]; edges: [string, string][] }> {
    const r = await this.send({ type: "GetGraph" });
    return r.type === "Graph" ? { nodes: r.nodes, edges: r.edges } : { nodes: [], edges: [] };
  }

  /** Insert a row after `index` (or at the top when `index` is null). */
  insertRow(index: number | null) {
    return this.send({ type: "InsertRow", index });
  }

  /** Delete the row currently at `index`. */
  deleteRow(index: number) {
    return this.send({ type: "DeleteRow", index });
  }

  /** Insert a column after `index` (or at the left edge when null). */
  insertColumn(index: number | null) {
    return this.send({ type: "InsertColumn", index });
  }

  /** Delete the column currently at `index`. */
  deleteColumn(index: number) {
    return this.send({ type: "DeleteColumn", index });
  }

  evaluate() {
    return this.send({ type: "Evaluate" });
  }

  applyOps(ops: Op[]) {
    return this.send({ type: "ApplyOps", ops });
  }

  reset() {
    return this.send({ type: "Reset" });
  }

  /** Snapshot the active sheet under a named version. */
  saveVersion(label: string) {
    return this.send({ type: "SaveVersion", label });
  }

  /** List saved versions as `[index, label, sheet]` tuples. */
  async listVersions(): Promise<[number, string, string][]> {
    const r = await this.send({ type: "ListVersions" });
    return r.type === "Versions" ? r.entries : [];
  }

  /** Diff two saved versions (left = before, right = after). */
  async diff(left: number, right: number): Promise<DiffRow[]> {
    const r = await this.send({ type: "Diff", left, right });
    return r.type === "Diff" ? r.rows : [];
  }

  /** Fork the active sheet into a new branch sheet. */
  fork(name: string) {
    return this.send({ type: "Fork", name });
  }

  /** Merge a branch sheet back into the sheet it was forked from. */
  async mergeBranch(
    name: string,
  ): Promise<{ applied: number; conflicts: MergeConflict[] } | null> {
    const r = await this.send({ type: "MergeBranch", name });
    return r.type === "Merge" ? { applied: r.applied, conflicts: r.conflicts } : null;
  }

  /** List branch sheets as `[name, parent]` pairs. */
  async listBranches(): Promise<[string, string][]> {
    const r = await this.send({ type: "ListBranches" });
    return r.type === "Branches" ? r.entries : [];
  }

  /** Load a sandboxed user-defined-function plugin from wasm bytes. */
  registerUdf(name: string, bytes: number[]) {
    return this.send({ type: "RegisterUDF", name, bytes });
  }

  /** Remove a previously loaded UDF plugin. */
  unregisterUdf(name: string) {
    return this.send({ type: "UnregisterUDF", name });
  }

  /** List the names of currently loaded UDF plugins. */
  async listUdfs(): Promise<string[]> {
    const r = await this.send({ type: "ListUDFs" });
    return r.type === "UDFs" ? r.names : [];
  }
}
