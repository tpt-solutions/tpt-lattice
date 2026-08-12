//! # tpt-lattice-wasm
//!
//! `wasm-bindgen` glue exposing the TPT Lattice engine to JavaScript. The
//! engine runs entirely inside a Web Worker; this crate is the worker's API
//! surface. Messages use JSON for ergonomics (a binary protocol can be layered
//! on top later).
//!
//! The protocol is a simple request/response envelope:
//!
//! ```text
//! { "type": "setCell", "cell": "A1", "value": { "Number": 42 } }
//! { "type": "setFormula", "cell": "B1", "formula": "=A1 * 2" }
//! { "type": "getCell", "cell": "B1" }            -> { "Number": 84 }
//! { "type": "evaluate" }
//! { "type": "applyOps", "ops": [ ... ] }          -> { "accepted": N }
//! ```

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};

use tpt_lattice_core::{CellId, CellValue};
use tpt_lattice_evaluator::Evaluator;
use wasm_bindgen::prelude::*;

use tpt_lattice_crdt::{ActorId, CrdtStore, Op};

/// A single request from the main thread to the engine worker.
///
/// Serialized with an externally-tagged `"type"` field so the JSON shape is
/// `{ "type": "SetCell", "cell": "A1", "value": { "Number": 42 } }`.
#[derive(serde::Deserialize)]
#[serde(tag = "type")]
pub enum Request {
    SetCell { cell: String, value: CellValue },
    SetFormula { cell: String, formula: String },
    GetCell { cell: String },
    Evaluate,
    ApplyOps { ops: Vec<Op> },
    Reset,
    /// (Re)assign this replica's actor id. Each collaborative client must use a
    /// distinct id so the CRDT's last-writer-wins rule is deterministic.
    Init { actor: ActorId },
    /// Delete a cell (authored op; recorded in the outbox for sync).
    DeleteCell { cell: String },
    /// Drain and return the ops this replica has authored since the last call.
    TakeOutbox,
    /// Return every materialized `(A1, value)` pair (for find/replace, copy).
    ListCells,
    /// Insert a row after `index` (or at the top when `index` is null).
    InsertRow { index: Option<u64> },
    /// Delete the row currently at `index`.
    DeleteRow { index: u64 },
    /// Insert a column after `index` (or at the left edge when null).
    InsertColumn { index: Option<u64> },
    /// Delete the column currently at `index`.
    DeleteColumn { index: u64 },
}

/// A response from the engine worker back to the main thread.
#[derive(serde::Serialize)]
#[serde(tag = "type")]
pub enum Response {
    Value { value: CellValue },
    Ok,
    Evaluated,
    OpsAccepted { count: usize },
    /// Locally-authored ops drained from the outbox.
    Outbox { ops: Vec<Op> },
    /// A materialized cell: its A1 address and current value.
    Cells { cells: Vec<CellListing> },
    Error { message: String },
}

/// A single materialized `(A1, value)` pair returned by [`Request::ListCells`].
#[derive(serde::Serialize)]
pub struct CellListing {
    pub cell: String,
    pub value: CellValue,
}

struct State {
    engine: Evaluator,
    crdt: CrdtStore,
    /// Ops authored locally since the last [`Request::TakeOutbox`], awaiting
    /// broadcast to peers.
    outbox: Vec<Op>,
}

/// The engine handle exposed to JS. Wraps the evaluator and a CRDT store.
#[wasm_bindgen]
pub struct LatticeEngine {
    state: Mutex<State>,
}

#[wasm_bindgen]
impl LatticeEngine {
    /// Create a new engine instance.
    #[wasm_bindgen(constructor)]
    pub fn new() -> LatticeEngine {
        LatticeEngine::default()
    }
}

impl Default for LatticeEngine {
    fn default() -> Self {
        LatticeEngine {
            state: Mutex::new(State {
                engine: Evaluator::new(),
                crdt: CrdtStore::new(1),
                outbox: Vec::new(),
            }),
        }
    }
}

#[wasm_bindgen]
impl LatticeEngine {
    /// Handle a JSON-encoded [`Request`], returning a JSON-encoded
    /// [`Response`]. Any error is returned as an `{ "type": "Error", ... }`.
    pub fn handle(&self, request_json: &str) -> String {
        let req: Request = match serde_json::from_str(request_json) {
            Ok(r) => r,
            Err(e) => return error_response(&e.to_string()),
        };
        let mut state = lock_state(&self.state);
        let resp = match req {
            Request::SetCell { cell, value } => match CellId::try_from_a1(&cell) {
                Ok(id) => {
                    let clock = state.crdt.clock().clone();
                    let actor = state.crdt.actor();
                    let v = value.clone();
                    let op = Op::SetCell {
                        cell: id,
                        value,
                        clock,
                        actor,
                    };
                    state.engine.set_value(id, v);
                    state.crdt.apply(op.clone());
                    state.outbox.push(op);
                    Response::Ok
                }
                Err(e) => Response::Error {
                    message: e.to_string(),
                },
            },
            Request::SetFormula { cell, formula } => match CellId::try_from_a1(&cell) {
                Ok(id) => match state.engine.recompute(id, &formula) {
                    Ok(_) => Response::Ok,
                    Err(e) => Response::Error {
                        message: e.to_string(),
                    },
                },
                Err(e) => Response::Error {
                    message: e.to_string(),
                },
            },
            Request::GetCell { cell } => match CellId::try_from_a1(&cell) {
                Ok(id) => Response::Value {
                    value: state.engine.get_value(id),
                },
                Err(e) => Response::Error {
                    message: e.to_string(),
                },
            },
            Request::Evaluate => match state.engine.evaluate() {
                Ok(_) => Response::Evaluated,
                Err(e) => Response::Error {
                    message: e.to_string(),
                },
            },
            Request::ApplyOps { ops } => {
                let count = ops.len();
                state.crdt.merge_ops(ops.iter().cloned());
                // Re-materialize CRDT values into the evaluator for display.
                state.engine = Evaluator::new();
                for (id, value) in crdt_cells(&state.crdt) {
                    state.engine.set_value(id, value);
                }
                Response::OpsAccepted { count }
            }
            Request::Reset => {
                *state = State {
                    engine: Evaluator::new(),
                    crdt: CrdtStore::new(state.crdt.actor()),
                    outbox: Vec::new(),
                };
                Response::Ok
            }
            Request::Init { actor } => {
                *state = State {
                    engine: Evaluator::new(),
                    crdt: CrdtStore::new(actor),
                    outbox: Vec::new(),
                };
                Response::Ok
            }
            Request::DeleteCell { cell } => match CellId::try_from_a1(&cell) {
                Ok(id) => {
                    let clock = state.crdt.clock().clone();
                    let actor = state.crdt.actor();
                    let op = Op::DeleteCell { cell: id, clock, actor };
                    state.crdt.apply(op.clone());
                    state.outbox.push(op);
                    // Re-materialize the evaluator from the CRDT.
                    state.engine = Evaluator::new();
                    for (cid, value) in crdt_cells(&state.crdt) {
                        state.engine.set_value(cid, value);
                    }
                    Response::Ok
                }
                Err(e) => Response::Error {
                    message: e.to_string(),
                },
            },
            Request::TakeOutbox => {
                let ops = std::mem::take(&mut state.outbox);
                Response::Outbox { ops }
            }
            Request::ListCells => {
                let cells = crdt_cells(&state.crdt)
                    .into_iter()
                    .map(|(id, value)| CellListing {
                        cell: id.to_a1(),
                        value,
                    })
                    .collect();
                Response::Cells { cells }
            }
            Request::InsertRow { index } => {
                let op = state.crdt.insert_row_at(index);
                state.outbox.push(op);
                // Structural edits shift cell content, so re-materialize the
                // evaluator from the rebuilt CRDT layout.
                state.engine = Evaluator::new();
                for (cid, value) in crdt_cells(&state.crdt) {
                    state.engine.set_value(cid, value);
                }
                Response::Ok
            }
            Request::DeleteRow { index } => {
                let op = state.crdt.delete_row_at(index);
                state.outbox.push(op);
                state.engine = Evaluator::new();
                for (cid, value) in crdt_cells(&state.crdt) {
                    state.engine.set_value(cid, value);
                }
                Response::Ok
            }
            Request::InsertColumn { index } => {
                let op = state.crdt.insert_column_at(index);
                state.outbox.push(op);
                state.engine = Evaluator::new();
                for (cid, value) in crdt_cells(&state.crdt) {
                    state.engine.set_value(cid, value);
                }
                Response::Ok
            }
            Request::DeleteColumn { index } => {
                let op = state.crdt.delete_column_at(index);
                state.outbox.push(op);
                state.engine = Evaluator::new();
                for (cid, value) in crdt_cells(&state.crdt) {
                    state.engine.set_value(cid, value);
                }
                Response::Ok
            }
        };
        serde_json::to_string(&resp).unwrap_or_else(|e| error_response(&e.to_string()))
    }
}

/// Convenience batch API: set many `(A1, JSON value)` pairs at once.
#[wasm_bindgen]
pub fn set_cells_json(engine: &LatticeEngine, cells_json: &str) -> String {
    let map: HashMap<String, CellValue> = match serde_json::from_str(cells_json) {
        Ok(m) => m,
        Err(e) => return error_response(&e.to_string()),
    };
    let mut state = lock_state(&engine.state);
    for (cell, value) in map {
        if let Ok(id) = CellId::try_from_a1(&cell) {
            let clock = state.crdt.clock().clone();
            let actor = state.crdt.actor();
            state.engine.set_value(id, value.clone());
            state.crdt.apply(Op::SetCell {
                cell: id,
                value,
                clock,
                actor,
            });
        }
    }
    serde_json::to_string(&Response::Ok).unwrap()
}

fn crdt_cells(crdt: &CrdtStore) -> Vec<(CellId, CellValue)> {
    // Materialize every non-empty cell from the CRDT store directly, rather than
    // probing a bounded coordinate window (which silently dropped cells outside
    // 0..1024 and rescanned ~1M coordinates on every mutation).
    crdt.iter_cells()
}

/// Acquire the engine lock, recovering from poisoning. A panic that occurred
/// while the lock was held would otherwise permanently brick the engine until a
/// full page reload; recovering lets the engine keep serving subsequent requests.
fn lock_state(state: &Mutex<State>) -> MutexGuard<'_, State> {
    match state.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn error_response(message: &str) -> String {
    serde_json::to_string(&Response::Error {
        message: message.to_string(),
    })
    .unwrap_or_else(|_| "{\"type\":\"Error\",\"message\":\"serialization failure\"}".to_string())
}

// Smoke tests executed by `wasm-pack test` in CI. Guarded to `wasm32` so the
// host `cargo test --workspace` run is unaffected (the crate is a `cdylib`).
#[cfg(all(test, target_arch = "wasm32"))]
mod wasm_tests {
    use tpt_lattice_core::{CellId, CellValue};
    use tpt_lattice_evaluator::Evaluator;
    use wasm_bindgen_test::wasm_bindgen_test;

    wasm_bindgen_test::wasm_bindgen_test_configure!();

    #[wasm_bindgen_test]
    fn engine_evaluates_through_handle() {
        let engine = super::LatticeEngine::new();
        engine.handle(r#"{"type":"SetCell","cell":"A1","value":{"Number":21}}"#);
        engine.handle(r#"{"type":"SetFormula","cell":"B1","formula":"=A1 * 2"}"#);
        engine.handle(r#"{"type":"Evaluate"}"#);
        let resp = engine.handle(r#"{"type":"GetCell","cell":"B1"}"#);
        assert!(resp.contains("\"Number\":42"), "expected 42, got {resp}");
    }

    #[wasm_bindgen_test]
    fn evaluator_core_math() {
        let mut e = Evaluator::new();
        e.set_value(CellId::from_a1("A1"), CellValue::Number(21.0));
        e.set_formula(CellId::from_a1("B1"), "=A1 * 2").unwrap();
        e.evaluate().unwrap();
        assert_eq!(e.get_value(CellId::from_a1("B1")), CellValue::Number(42.0));
    }
}
