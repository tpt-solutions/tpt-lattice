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
use std::sync::Mutex;

use tpt_lattice_core::{CellId, CellValue};
use tpt_lattice_evaluator::Evaluator;
use wasm_bindgen::prelude::*;

use tpt_lattice_crdt::{CrdtStore, Op};

/// A single request from the main thread to the engine worker.
#[derive(serde::Deserialize)]
pub enum Request {
    SetCell { cell: String, value: CellValue },
    SetFormula { cell: String, formula: String },
    GetCell { cell: String },
    Evaluate,
    ApplyOps { ops: Vec<Op> },
    Reset,
}

/// A response from the engine worker back to the main thread.
#[derive(serde::Serialize)]
#[serde(tag = "type")]
pub enum Response {
    Value { value: CellValue },
    Ok,
    Evaluated,
    OpsAccepted { count: usize },
    Error { message: String },
}

struct State {
    engine: Evaluator,
    crdt: CrdtStore,
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
        LatticeEngine {
            state: Mutex::new(State {
                engine: Evaluator::new(),
                crdt: CrdtStore::new(1),
            }),
        }
    }

    /// Handle a JSON-encoded [`Request`], returning a JSON-encoded
    /// [`Response`]. Any error is returned as an `{ "type": "Error", ... }`.
    pub fn handle(&self, request_json: &str) -> String {
        let req: Request = match serde_json::from_str(request_json) {
            Ok(r) => r,
            Err(e) => return error_response(&e.to_string()),
        };
        let mut state = self.state.lock().unwrap();
        let resp = match req {
            Request::SetCell { cell, value } => match CellId::try_from_a1(&cell) {
                Ok(id) => {
                    let clock = state.crdt.clock().clone();
                    let actor = state.crdt.actor();
                    state.engine.set_value(id, value.clone());
                    state.crdt.apply(Op::SetCell {
                        cell: id,
                        value,
                        clock,
                        actor,
                    });
                    Response::Ok
                }
                Err(e) => Response::Error { message: e.to_string() },
            },
            Request::SetFormula { cell, formula } => match CellId::try_from_a1(&cell) {
                Ok(id) => match state.engine.recompute(id, &formula) {
                    Ok(_) => Response::Ok,
                    Err(e) => Response::Error { message: e.to_string() },
                },
                Err(e) => Response::Error { message: e.to_string() },
            },
            Request::GetCell { cell } => match CellId::try_from_a1(&cell) {
                Ok(id) => Response::Value {
                    value: state.engine.get_value(id),
                },
                Err(e) => Response::Error { message: e.to_string() },
            },
            Request::Evaluate => match state.engine.evaluate() {
                Ok(_) => Response::Evaluated,
                Err(e) => Response::Error { message: e.to_string() },
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
                };
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
    let mut state = engine.state.lock().unwrap();
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
    // Drain all non-empty cells from the CRDT store via its GridState impl.
    let mut out = Vec::new();
    for row in 0..1024u64 {
        for col in 0..1024u64 {
            let id = CellId::from_rc(col, row);
            let v = crdt.get_cell(id);
            if !v.is_empty() {
                out.push((id, v));
            }
        }
    }
    out
}

fn error_response(message: &str) -> String {
    serde_json::to_string(&Response::Error {
        message: message.to_string(),
    })
    .unwrap_or_else(|_| "{\"type\":\"Error\",\"message\":\"serialization failure\"}".to_string())
}
