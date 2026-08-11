import type { Op } from "../types";

const DB_NAME = "tpt-lattice-sync";
const STORE = "ops";
const VERSION = 1;

/**
 * Persistent, ordered log of locally-authored ops, backed by IndexedDB so edits
 * survive page reloads and can be replayed on reconnect. Only ops this replica
 * authored are stored here; remote ops are applied but never enqueued (that would
 * create broadcast loops).
 */
export class OpLog {
  private dbPromise: Promise<IDBDatabase> | null = null;

  private open(): Promise<IDBDatabase> {
    if (this.dbPromise) return this.dbPromise;
    this.dbPromise = new Promise((resolve, reject) => {
      const req = indexedDB.open(DB_NAME, VERSION);
      req.onupgradeneeded = () => {
        const db = req.result;
        if (!db.objectStoreNames.contains(STORE)) {
          db.createObjectStore(STORE, { keyPath: "seq", autoIncrement: true });
        }
      };
      req.onsuccess = () => resolve(req.result);
      req.onerror = () => reject(req.error);
    });
    return this.dbPromise;
  }

  private tx(db: IDBDatabase, mode: IDBTransactionMode) {
    return db.transaction(STORE, mode).objectStore(STORE);
  }

  /** Append an op to the persistent log. */
  async add(op: Op): Promise<void> {
    const db = await this.open();
    return new Promise((resolve, reject) => {
      const store = this.tx(db, "readwrite");
      // Strip any prior `seq` so auto-increment assigns a fresh key.
      const { seq: _seq, ...rest } = op as Op & { seq?: number };
      store.add(rest);
      store.transaction.oncomplete = () => resolve();
      store.transaction.onerror = () => reject(store.transaction.error);
    });
  }

  /** All locally-authored ops, in insertion order. */
  async all(): Promise<Op[]> {
    const db = await this.open();
    return new Promise((resolve, reject) => {
      const store = this.tx(db, "readonly");
      const req = store.getAll();
      req.onsuccess = () => resolve((req.result as Op[]) ?? []);
      req.onerror = () => reject(req.error);
    });
  }

  async clear(): Promise<void> {
    const db = await this.open();
    return new Promise((resolve, reject) => {
      const store = this.tx(db, "readwrite");
      store.clear();
      store.transaction.oncomplete = () => resolve();
      store.transaction.onerror = () => reject(store.transaction.error);
    });
  }
}
