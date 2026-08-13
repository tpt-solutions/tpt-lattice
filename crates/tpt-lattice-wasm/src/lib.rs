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
//!
//! The engine supports multiple sheets. Every request operates on the active
//! sheet; `newSheet` / `deleteSheet` / `renameSheet` / `selectSheet` manage them.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};

use tpt_lattice_core::{CellId, CellValue, GridState};
use tpt_lattice_evaluator::{diff, three_way_merge, DiffKind, Evaluator, SheetSnapshot};
use wasm_bindgen::prelude::*;

use tpt_lattice_crdt::{ActorId, CrdtStore, Op};

mod plugin;

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
    /// Create a new (empty) sheet with the given name.
    NewSheet { name: String },
    /// Delete a sheet by name (refused when it is the last sheet).
    DeleteSheet { name: String },
    /// Rename a sheet (`from` -> `to`).
    RenameSheet { from: String, to: String },
    /// Make `name` the active sheet for subsequent requests.
    SelectSheet { name: String },
    /// List sheet names and the active sheet.
    ListSheets,
    /// Return the active sheet's dependency graph (DAG) as A1 nodes + edges.
    GetGraph,
    /// Define or overwrite a named range / reusable formula on the active sheet.
    SetNamed { name: String, expr: String },
    /// Remove a named range from the active sheet.
    ClearNamed { name: String },
    /// List the active sheet's named ranges as `(name, expression)` pairs.
    ListNamed,
    /// Capture the active sheet's current state as a named history checkpoint.
    Checkpoint { label: String },
    /// List the recorded history checkpoints as `(index, label)` pairs.
    ListHistory,
    /// Restore the active sheet to a previously captured checkpoint.
    Restore { index: usize },
    /// Snapshot the active sheet under a named version for later diffing/merging.
    SaveVersion { label: String },
    /// List saved versions as `(index, label, sheet)` tuples.
    ListVersions,
    /// Diff two saved versions (by index), left = before, right = after.
    Diff { left: usize, right: usize },
    /// Fork the active sheet into a new branch sheet for safe experimentation.
    Fork { name: String },
    /// Merge a branch sheet back into the sheet it was forked from.
    MergeBranch { name: String },
    /// List branch sheets as `(name, parent)` pairs.
    ListBranches,
    /// Load a sandboxed user-defined-function plugin from wasm bytes.
    RegisterUDF { name: String, bytes: Vec<u8> },
    /// Remove a previously loaded UDF plugin by name.
    UnregisterUDF { name: String },
    /// List the names of currently loaded UDF plugins.
    ListUDFs,
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
    /// Sheet names and the active sheet.
    Sheets { sheets: Vec<String>, active: String },
    /// The dependency graph: node A1 labels and `(dependency, dependent)` edges.
    Graph { nodes: Vec<String>, edges: Vec<(String, String)> },
    /// The named ranges of the active sheet.
    Named { names: Vec<(String, String)> },
    /// The recorded history checkpoints as `(index, label)` pairs.
    History { entries: Vec<(usize, String)> },
    /// Saved versions as `(index, label, sheet)` tuples.
    Versions { entries: Vec<(usize, String, String)> },
    /// A cell-level diff between two versions.
    Diff { rows: Vec<DiffRowJSON> },
    /// Branch sheets as `(name, parent)` pairs.
    Branches { entries: Vec<(String, String)> },
    /// The result of merging a branch back: how many cells auto-applied, and any
    /// conflicts that need manual resolution.
    Merge { applied: usize, conflicts: Vec<MergeConflictJSON> },
    /// The names of currently loaded UDF plugins.
    UDFs { names: Vec<String> },
    Error { message: String },
}

/// A single materialized `(A1, value)` pair returned by [`Request::ListCells`].
#[derive(serde::Serialize)]
pub struct CellListing {
    pub cell: String,
    pub value: CellValue,
}

/// A cell's value + (optional) formula, as shown in a diff/merge report.
#[derive(serde::Serialize)]
pub struct DiffCellJSON {
    pub value: Option<CellValue>,
    pub formula: Option<String>,
}

/// One row of a version diff: the cell, the change kind, and the left/right
/// sides (either may be `null` for added/removed cells).
#[derive(serde::Serialize)]
pub struct DiffRowJSON {
    pub cell: String,
    pub status: String,
    pub left: Option<DiffCellJSON>,
    pub right: Option<DiffCellJSON>,
}

/// A cell that could not be auto-merged (both sides changed differently).
#[derive(serde::Serialize)]
pub struct MergeConflictJSON {
    pub cell: String,
    pub base: Option<DiffCellJSON>,
    pub ours: Option<DiffCellJSON>,
    pub theirs: Option<DiffCellJSON>,
}

/// All state for a single sheet: its evaluator, its CRDT, and the ops authored
/// locally since the last sync drain.
struct SheetState {
    engine: Evaluator,
    crdt: CrdtStore,
    outbox: Vec<Op>,
}

impl Default for SheetState {
    fn default() -> Self {
        let mut engine = Evaluator::new();
        plugin::register_plugins_on(&mut engine);
        SheetState {
            engine,
            crdt: CrdtStore::new(1),
            outbox: Vec::new(),
        }
    }
}

impl SheetState {
    /// Rebuild the evaluator from the CRDT's materialized cells (used after any
    /// structural edit or remote op merge, since those shift/rewrite content).
    fn rematerialize(&mut self) {
        self.engine = Evaluator::new();
        for (id, value) in crdt_cells(&self.crdt) {
            self.engine.set_value(id, value);
        }
    }
}

/// A captured state of a single sheet, sufficient to restore it later. Built on
/// the requirement that time-travel should let a user scrub back through edit
/// history and restore a prior state.
#[derive(Clone)]
struct Snapshot {
    label: String,
    cells: Vec<(CellId, CellValue)>,
    formulas: Vec<(CellId, String)>,
}

/// A named, saved point-in-time capture of a sheet, used for diffing and as the
/// common ancestor when merging a branch back.
#[derive(Clone)]
struct VersionEntry {
    label: String,
    sheet: String,
    snap: SheetSnapshot,
}

/// Metadata for a forked ("what-if") branch sheet: which sheet it was forked
/// from, and the snapshot of that parent at fork time (the 3-way merge base).
#[derive(Clone)]
struct BranchMeta {
    parent: String,
    base: SheetSnapshot,
}

struct State {
    sheets: HashMap<String, SheetState>,
    active: String,
    history: Vec<Snapshot>,
    history_seq: u64,
    /// Named saved versions of sheets, for Git-style diff/merge views.
    versions: Vec<VersionEntry>,
    /// Branch sheets and their fork metadata, for "what-if" experimentation.
    branches: HashMap<String, BranchMeta>,
}

impl Default for State {
    fn default() -> Self {
        State {
            sheets: HashMap::new(),
            active: String::new(),
            history: Vec::new(),
            history_seq: 0,
            versions: Vec::new(),
            branches: HashMap::new(),
        }
    }
}

/// The engine handle exposed to JS. Wraps a map of sheets, each with its own
/// evaluator and CRDT store.
#[wasm_bindgen]
pub struct LatticeEngine {
    state: Mutex<State>,
}

#[wasm_bindgen]
impl LatticeEngine {
    /// Create a new engine instance (a single `Sheet1`).
    #[wasm_bindgen(constructor)]
    pub fn new() -> LatticeEngine {
        LatticeEngine::default()
    }
}

impl Default for LatticeEngine {
    fn default() -> Self {
        let mut sheets = HashMap::new();
        sheets.insert("Sheet1".to_string(), SheetState::default());
        LatticeEngine {
            state: Mutex::new(State {
                sheets,
                active: "Sheet1".to_string(),
                history: Vec::new(),
                history_seq: 0,
                versions: Vec::new(),
                branches: HashMap::new(),
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
        // Clone the active sheet name up front: the borrow checker rejects
        // `state.sheets.get_mut(&active)` (shared + mutable borrows of the
        // same struct). Using a local copy keeps the two borrows disjoint.
        let active = state.active.clone();
        // Mutations that change the active sheet's contents should be captured
        // in the time-travel history so users can scrub back to them.
        let is_mutating = matches!(
            req,
            Request::SetCell { .. }
                | Request::SetFormula { .. }
                | Request::DeleteCell { .. }
                | Request::ApplyOps { .. }
                | Request::Reset
                | Request::InsertRow { .. }
                | Request::DeleteRow { .. }
                | Request::InsertColumn { .. }
                | Request::DeleteColumn { .. }
                | Request::Fork { .. }
                | Request::MergeBranch { .. }
        );
        let resp = match req {
            Request::SetCell { cell, value } => match CellId::try_from_a1(&cell) {
                Ok(id) => {
                    let s = state.sheets.get_mut(&active).unwrap();
                    let clock = s.crdt.clock().clone();
                    let actor = s.crdt.actor();
                    let v = value.clone();
                    let op = Op::SetCell {
                        cell: id,
                        value,
                        clock,
                        actor,
                    };
                    s.engine.set_value(id, v);
                    s.crdt.apply(op.clone());
                    s.outbox.push(op);
                    Response::Ok
                }
                Err(e) => Response::Error {
                    message: e.to_string(),
                },
            },
            Request::SetFormula { cell, formula } => match CellId::try_from_a1(&cell) {
                Ok(id) => {
                    let s = state.sheets.get_mut(&active).unwrap();
                    match s.engine.recompute(id, &formula) {
                        Ok(_) => Response::Ok,
                        Err(e) => Response::Error {
                            message: e.to_string(),
                        },
                    }
                }
                Err(e) => Response::Error {
                    message: e.to_string(),
                },
            },
            Request::GetCell { cell } => match CellId::try_from_a1(&cell) {
                Ok(id) => {
                    let s = state.sheets.get(&active).unwrap();
                    Response::Value {
                        value: s.engine.get_value(id),
                    }
                }
                Err(e) => Response::Error {
                    message: e.to_string(),
                },
            },
            Request::Evaluate => {
                // Expose every other sheet's current values so cross-sheet
                // references (e.g. `Sheet2!A1`) resolve during evaluation.
                let views = all_sheet_views(&state, &active);
                let s = state.sheets.get_mut(&active).unwrap();
                s.engine.set_sheet_views(views);
                match s.engine.evaluate() {
                    Ok(_) => Response::Evaluated,
                    Err(e) => Response::Error {
                        message: e.to_string(),
                    },
                }
            }
            Request::ApplyOps { ops } => {
                let s = state.sheets.get_mut(&active).unwrap();
                s.crdt.merge_ops(ops.iter().cloned());
                // Re-materialize CRDT values into the evaluator for display.
                s.rematerialize();
                Response::OpsAccepted { count: ops.len() }
            }
            Request::Reset => {
                let s = state.sheets.get_mut(&active).unwrap();
                *s = SheetState::default();
                Response::Ok
            }
            Request::Init { actor } => {
                *state = State {
                    sheets: {
                        let mut m = HashMap::new();
                        m.insert("Sheet1".to_string(), SheetState::default_with_actor(actor));
                        m
                    },
                    active: "Sheet1".to_string(),
                    history: Vec::new(),
                    history_seq: 0,
                    versions: Vec::new(),
                    branches: HashMap::new(),
                };
                Response::Ok
            }
            Request::DeleteCell { cell } => match CellId::try_from_a1(&cell) {
                Ok(id) => {
                    let s = state.sheets.get_mut(&active).unwrap();
                    let clock = s.crdt.clock().clone();
                    let actor = s.crdt.actor();
                    let op = Op::DeleteCell { cell: id, clock, actor };
                    s.crdt.apply(op.clone());
                    s.outbox.push(op);
                    // Re-materialize the evaluator from the CRDT.
                    s.rematerialize();
                    Response::Ok
                }
                Err(e) => Response::Error {
                    message: e.to_string(),
                },
            },
            Request::TakeOutbox => {
                let s = state.sheets.get_mut(&active).unwrap();
                let ops = std::mem::take(&mut s.outbox);
                Response::Outbox { ops }
            }
            Request::ListCells => {
                let s = state.sheets.get(&active).unwrap();
                let cells = crdt_cells(&s.crdt)
                    .into_iter()
                    .map(|(id, value)| CellListing {
                        cell: id.to_a1(),
                        value,
                    })
                    .collect();
                Response::Cells { cells }
            }
            Request::InsertRow { index } => {
                let s = state.sheets.get_mut(&active).unwrap();
                let op = s.crdt.insert_row_at(index);
                s.outbox.push(op);
                s.rematerialize();
                Response::Ok
            }
            Request::DeleteRow { index } => {
                let s = state.sheets.get_mut(&active).unwrap();
                let op = s.crdt.delete_row_at(index);
                s.outbox.push(op);
                s.rematerialize();
                Response::Ok
            }
            Request::InsertColumn { index } => {
                let s = state.sheets.get_mut(&active).unwrap();
                let op = s.crdt.insert_column_at(index);
                s.outbox.push(op);
                s.rematerialize();
                Response::Ok
            }
            Request::DeleteColumn { index } => {
                let s = state.sheets.get_mut(&active).unwrap();
                let op = s.crdt.delete_column_at(index);
                s.outbox.push(op);
                s.rematerialize();
                Response::Ok
            }
            Request::NewSheet { name } => {
                if state.sheets.contains_key(&name) {
                    return error_response(&format!("sheet '{name}' already exists"));
                }
                state.sheets.insert(name.clone(), SheetState::default());
                Response::Ok
            }
            Request::DeleteSheet { name } => {
                if state.sheets.len() <= 1 {
                    return error_response("cannot delete the last sheet");
                }
                state.sheets.remove(&name);
                if state.active == name {
                    state.active = state.sheets.keys().next().cloned().unwrap();
                }
                Response::Ok
            }
            Request::RenameSheet { from, to } => {
                if !state.sheets.contains_key(&from) {
                    return error_response(&format!("no such sheet '{from}'"));
                }
                if state.sheets.contains_key(&to) {
                    return error_response(&format!("sheet '{to}' already exists"));
                }
                let st = state.sheets.remove(&from).unwrap();
                state.sheets.insert(to.clone(), st);
                if state.active == from {
                    state.active = to;
                }
                Response::Ok
            }
            Request::SelectSheet { name } => {
                if !state.sheets.contains_key(&name) {
                    return error_response(&format!("no such sheet '{name}'"));
                }
                state.active = name;
                Response::Ok
            }
            Request::ListSheets => {
                let sheets: Vec<String> = state.sheets.keys().cloned().collect();
                Response::Sheets {
                    sheets,
                    active: state.active.clone(),
                }
            }
            Request::GetGraph => {
                let s = state.sheets.get(&active).unwrap();
                let nodes: Vec<String> = crdt_cells(&s.crdt)
                    .into_iter()
                    .map(|(id, _)| id.to_a1())
                    .collect();
                let edges: Vec<(String, String)> = s
                    .engine
                    .dag()
                    .edges()
                    .into_iter()
                    .map(|(a, b)| (a.to_a1(), b.to_a1()))
                    .collect();
                Response::Graph { nodes, edges }
            }
            Request::SetNamed { name, expr } => {
                let s = state.sheets.get_mut(&active).unwrap();
                s.engine.set_named_range(&name, &expr);
                Response::Ok
            }
            Request::ClearNamed { name } => {
                let s = state.sheets.get_mut(&active).unwrap();
                s.engine.clear_named_range(&name);
                Response::Ok
            }
            Request::ListNamed => {
                let s = state.sheets.get(&active).unwrap();
                let names = s.engine.list_named_ranges();
                Response::Named { names }
            }
            Request::Checkpoint { label } => {
                record_checkpoint(&mut state, &active, label);
                Response::Ok
            }
            Request::ListHistory => {
                let entries: Vec<(usize, String)> = state
                    .history
                    .iter()
                    .enumerate()
                    .map(|(i, s)| (i, s.label.clone()))
                    .collect();
                Response::History { entries }
            }
            Request::Restore { index } => {
                if let Some(snap) = state.history.get(index).cloned() {
                    if let Some(s) = state.sheets.get_mut(&active) {
                        restore_snapshot(s, &snap);
                    }
                    Response::Ok
                } else {
                    Response::Error {
                        message: format!("no checkpoint at index {index}"),
                    }
                }
            }
            Request::SaveVersion { label } => {
                let active = state.active.clone();
                let snap = state.sheets.get(&active).unwrap().engine.snapshot();
                state
                    .versions
                    .push(VersionEntry { label, sheet: active.clone(), snap });
                Response::Ok
            }
            Request::ListVersions => {
                let entries: Vec<(usize, String, String)> = state
                    .versions
                    .iter()
                    .enumerate()
                    .map(|(i, v)| (i, v.label.clone(), v.sheet.clone()))
                    .collect();
                Response::Versions { entries }
            }
            Request::Diff { left, right } => {
                let (a, b) = match (state.versions.get(left), state.versions.get(right)) {
                    (Some(a), Some(b)) => (&a.snap, &b.snap),
                    _ => {
                        return error_response(&format!(
                            "no such version index (left={left}, right={right})"
                        ))
                    }
                };
                let rows = diff(a, b)
                    .into_iter()
                    .map(|r| DiffRowJSON {
                        cell: r.cell.to_a1(),
                        status: diff_status(r.kind).to_string(),
                        left: r.left.as_ref().map(to_diff_cell_json),
                        right: r.right.as_ref().map(to_diff_cell_json),
                    })
                    .collect();
                Response::Diff { rows }
            }
            Request::Fork { name } => {
                if state.sheets.contains_key(&name) {
                    return error_response(&format!("sheet '{name}' already exists"));
                }
                let parent = state.active.clone();
                let base = state.sheets.get(&parent).unwrap().engine.snapshot();
                let src = state.sheets.get(&parent).unwrap();
                let mut fork_engine = src.engine.clone();
                plugin::register_plugins_on(&mut fork_engine);
                let fork_crdt = rebuild_crdt(&fork_engine, src.crdt.actor());
                state.sheets.insert(
                    name.clone(),
                    SheetState {
                        engine: fork_engine,
                        crdt: fork_crdt,
                        outbox: Vec::new(),
                    },
                );
                state
                    .branches
                    .insert(name.clone(), BranchMeta { parent, base });
                Response::Ok
            }
            Request::MergeBranch { name } => {
                let meta = match state.branches.get(&name) {
                    Some(m) => m.clone(),
                    None => return error_response(&format!("no such branch '{name}'")),
                };
                let parent = meta.parent.clone();
                let base = meta.base.clone();
                let ours = state.sheets.get(&parent).unwrap().engine.snapshot();
                let theirs = state.sheets.get(&name).unwrap().engine.snapshot();
                let result = three_way_merge(&base, &ours, &theirs);
                {
                    let p = state.sheets.get_mut(&parent).unwrap();
                    for (id, cs) in &result.applied {
                        if let Some(f) = &cs.formula {
                            let _ = p.engine.set_formula(*id, f);
                        } else {
                            p.engine.set_value(*id, cs.value.clone());
                        }
                    }
                    let _ = p.engine.evaluate();
                    p.crdt = rebuild_crdt(&p.engine, p.crdt.actor());
                    p.outbox.clear();
                }
                // Re-base the branch on the merged parent so a later merge
                // treats subsequent branch edits as fresh changes.
                if let Some(m) = state.branches.get_mut(&name) {
                    m.base = ours;
                }
                let conflicts = result
                    .conflicts
                    .into_iter()
                    .map(|c| MergeConflictJSON {
                        cell: c.cell.to_a1(),
                        base: c.base.as_ref().map(to_diff_cell_json),
                        ours: c.ours.as_ref().map(to_diff_cell_json),
                        theirs: c.theirs.as_ref().map(to_diff_cell_json),
                    })
                    .collect();
                Response::Merge {
                    applied: result.applied.len(),
                    conflicts,
                }
            }
            Request::ListBranches => {
                let entries: Vec<(String, String)> = state
                    .branches
                    .iter()
                    .map(|(name, m)| (name.clone(), m.parent.clone()))
                    .collect();
                Response::Branches { entries }
            }
            Request::RegisterUDF { name, bytes } => {
                match plugin::register_plugin(&name, &bytes) {
                    Ok(()) => {
                        for s in state.sheets.values_mut() {
                            plugin::register_plugins_on(&mut s.engine);
                        }
                        Response::Ok
                    }
                    Err(e) => Response::Error { message: e },
                }
            }
            Request::UnregisterUDF { name } => {
                plugin::unregister_plugin(&name);
                for s in state.sheets.values_mut() {
                    s.engine.remove_external(&name);
                }
                Response::Ok
            }
            Request::ListUDFs => Response::UDFs {
                names: plugin::list_plugins(),
            },
        };
        if is_mutating {
            let seq = state.history_seq;
            record_checkpoint(&mut state, &active, format!("Edit {seq}"));
            state.history_seq += 1;
        }
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
    let active = state.active.clone();
    let s = state.sheets.get_mut(&active).unwrap();
    for (cell, value) in map {
        if let Ok(id) = CellId::try_from_a1(&cell) {
            let clock = s.crdt.clock().clone();
            let actor = s.crdt.actor();
            s.engine.set_value(id, value.clone());
            s.crdt.apply(Op::SetCell {
                cell: id,
                value,
                clock,
                actor,
            });
        }
    }
    serde_json::to_string(&Response::Ok).unwrap()
}

impl SheetState {
    fn default_with_actor(actor: ActorId) -> Self {
        let mut engine = Evaluator::new();
        plugin::register_plugins_on(&mut engine);
        SheetState {
            engine,
            crdt: CrdtStore::new(actor),
            outbox: Vec::new(),
        }
    }
}

fn crdt_cells(crdt: &CrdtStore) -> Vec<(CellId, CellValue)> {
    // Materialize every non-empty cell from the CRDT store directly, rather than
    // probing a bounded coordinate window (which silently dropped cells outside
    // 0..1024 and rescanned ~1m coordinates on every mutation).
    crdt.iter_cells()
}

/// Build a `sheet name -> (cell id -> value)` map of every sheet except the
/// active one, used to resolve cross-sheet references during evaluation.
fn all_sheet_views(
    state: &State,
    active: &str,
) -> HashMap<String, HashMap<CellId, CellValue>> {
    let mut views = HashMap::new();
    for (name, sheet) in &state.sheets {
        if name == active {
            continue;
        }
        let cells: HashMap<CellId, CellValue> = crdt_cells(&sheet.crdt).into_iter().collect();
        views.insert(name.clone(), cells);
    }
    views
}

/// Maximum number of checkpoints retained in the edit-history timeline.
const MAX_HISTORY: usize = 200;

/// Capture the active sheet's current cells + formulas as a checkpoint.
fn record_checkpoint(state: &mut State, active: &str, label: String) {
    let (cells, formulas) = match state.sheets.get(active) {
        Some(s) => (crdt_cells(&s.crdt), s.engine.list_formulas()),
        None => return,
    };
    state.history.push(Snapshot { label, cells, formulas });
    if state.history.len() > MAX_HISTORY {
        state.history.remove(0);
    }
}

/// Rebuild a sheet's evaluator and CRDT from a captured checkpoint.
fn restore_snapshot(sheet: &mut SheetState, snap: &Snapshot) {
    let mut engine = Evaluator::new();
    for (id, v) in &snap.cells {
        engine.set_value(*id, v.clone());
    }
    for (id, f) in &snap.formulas {
        let _ = engine.set_formula(*id, f);
    }
    let _ = engine.evaluate();
    sheet.engine = engine;
    // Plugins live in the (cloned-away) old engine; re-bind them onto the
    // freshly built one so UDFs keep working after a restore.
    plugin::register_plugins_on(&mut sheet.engine);

    let mut crdt = CrdtStore::new(sheet.crdt.actor());
    for (id, v) in &snap.cells {
        let clock = crdt.clock().clone();
        let actor = crdt.actor();
        crdt.apply(Op::SetCell {
            cell: *id,
            value: v.clone(),
            clock,
            actor,
        });
    }
    sheet.crdt = crdt;
    sheet.outbox.clear();
}

/// Map a [`DiffKind`] to its JSON `status` string for the diff report.
fn diff_status(kind: DiffKind) -> &'static str {
    match kind {
        DiffKind::Added => "added",
        DiffKind::Removed => "removed",
        DiffKind::Changed => "changed",
        DiffKind::Unchanged => "unchanged",
    }
}

/// Convert a [`CellSnapshot`] into its JSON representation (empty values are
/// serialized as `null` so the UI can treat them as "no cell").
fn to_diff_cell_json(c: &tpt_lattice_evaluator::CellSnapshot) -> DiffCellJSON {
    DiffCellJSON {
        value: if c.value.is_empty() { None } else { Some(c.value.clone()) },
        formula: c.formula.clone(),
    }
}

/// Rebuild a sheet's CRDT from the evaluator's current cells, preserving the
/// actor id. Used after a fork (to copy the parent's content) and after a
/// branch merge (so the applied cells are reflected in the CRDT/op log).
fn rebuild_crdt(engine: &Evaluator, actor: ActorId) -> CrdtStore {
    let mut crdt = CrdtStore::new(actor);
    for (id, v) in engine.iter_cells() {
        let clock = crdt.clock().clone();
        crdt.apply(Op::SetCell {
            cell: id,
            value: v,
            clock,
            actor,
        });
    }
    crdt
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
use tpt_lattice_core::{CellId, CellValue, GridState};
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
