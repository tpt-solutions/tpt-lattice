//! # tpt-lattice-crdt
//!
//! An operation-based CRDT for conflict-free, offline-first grid mutations.
//!
//! Every mutation is expressed as an [`Op`] carrying a causal [`VectorClock`]
//! and the originating actor id. Applying ops uses a deterministic
//! last-writer-wins rule keyed on `(clock_total, actor)`, which makes merge
//! **commutative** and **associative** — any two peers that have seen the same
//! set of ops converge to the identical state, regardless of arrival order.
//!
//! Row and column insertions carry immutable [`ulid::Ulid`] identifiers (never
//! integer indices) so that concurrent edits to the same region cannot corrupt
//! each other.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde::{Deserialize, Serialize};
use tpt_lattice_core::{CellId, CellValue};
use ulid::Ulid;

/// Identifies a replica. In a real deployment this is a stable per-client UUID;
/// for the CRDT math any total orderable id works.
pub type ActorId = u64;

/// A version vector mapping each actor to the highest sequence number it has
/// observed from that actor. Used for causal ordering and divergence detection.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct VectorClock {
    entries: BTreeMap<ActorId, u64>,
}

impl VectorClock {
    /// An empty clock.
    pub fn new() -> Self {
        VectorClock::default()
    }

    /// Increment `actor`'s entry by one and return the new clock (this is the
    /// timestamp stamped onto a freshly authored op).
    pub fn tick(&mut self, actor: ActorId) -> VectorClock {
        let e = self.entries.entry(actor).or_insert(0);
        *e += 1;
        self.clone()
    }

    /// Merge another clock into this one, taking the per-actor maximum.
    pub fn merge(&mut self, other: &VectorClock) {
        for (actor, seq) in &other.entries {
            let e = self.entries.entry(*actor).or_insert(0);
            *e = (*e).max(*seq);
        }
    }

    /// Whether `self` happens-before `other` (every entry <=, at least one <).
    pub fn happens_before(&self, other: &VectorClock) -> bool {
        let mut strictly_less = false;
        let all_keys: std::collections::BTreeSet<&ActorId> =
            self.entries.keys().chain(other.entries.keys()).collect();
        for k in all_keys {
            let a = self.entries.get(k).copied().unwrap_or(0);
            let b = other.entries.get(k).copied().unwrap_or(0);
            if a > b {
                return false;
            }
            if a < b {
                strictly_less = true;
            }
        }
        strictly_less
    }

    /// Whether the two clocks are concurrent (incomparable).
    pub fn concurrent(&self, other: &VectorClock) -> bool {
        !self.happens_before(other) && !other.happens_before(self)
    }

    /// A scalar used purely for deterministic last-writer-wins tie-breaking.
    fn total(&self) -> u64 {
        self.entries.values().sum()
    }
}

/// A single replicated mutation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Op {
    /// Set a cell to a value.
    SetCell {
        /// Target cell.
        cell: CellId,
        /// New value.
        value: CellValue,
        /// Causal timestamp.
        clock: VectorClock,
        /// Authoring actor.
        actor: ActorId,
    },
    /// Clear a cell.
    DeleteCell {
        /// Target cell.
        cell: CellId,
        /// Causal timestamp.
        clock: VectorClock,
        /// Authoring actor.
        actor: ActorId,
    },
    /// Insert a new row with an immutable id, after `after` (or at the top).
    InsertRow {
        /// Immutable row id.
        id: Ulid,
        /// Row to insert after, if any.
        after: Option<Ulid>,
        /// Causal timestamp.
        clock: VectorClock,
        /// Authoring actor.
        actor: ActorId,
    },
    /// Insert a new column with an immutable id.
    InsertColumn {
        /// Immutable column id.
        id: Ulid,
        /// Column to insert after, if any.
        after: Option<Ulid>,
        /// Causal timestamp.
        clock: VectorClock,
        /// Authoring actor.
        actor: ActorId,
    },
    /// Delete a row by its immutable id.
    DeleteRow {
        /// Row id to delete.
        id: Ulid,
        /// Causal timestamp.
        clock: VectorClock,
        /// Authoring actor.
        actor: ActorId,
    },
    /// Delete a column by its immutable id.
    DeleteColumn {
        /// Column id to delete.
        id: Ulid,
        /// Causal timestamp.
        clock: VectorClock,
        /// Authoring actor.
        actor: ActorId,
    },
}

/// Deterministic precedence for last-writer-wins: higher clock total wins, with
/// actor id as a stable tie-breaker for concurrent ops.
fn precedence(op: &Op) -> (u64, ActorId) {
    let (clock, actor) = match op {
        Op::SetCell { clock, actor, .. }
        | Op::DeleteCell { clock, actor, .. }
        | Op::InsertRow { clock, actor, .. }
        | Op::InsertColumn { clock, actor, .. }
        | Op::DeleteRow { clock, actor, .. }
        | Op::DeleteColumn { clock, actor, .. } => (clock, *actor),
    };
    (clock.total(), actor)
}

/// Marker used to derive a deterministic, immutable "virtual" row/column id for
/// positions that have never been explicitly inserted. Real ULIDs carry a
/// ~current-millisecond timestamp in their high bits, so encoding the kind in the
/// timestamp field keeps virtual ids from ever colliding with real ones.
const KIND_ROW: u64 = 1;
const KIND_COL: u64 = 2;

fn virtual_ulid(kind: u64, index: u64) -> Ulid {
    Ulid::from_parts(kind, index as u128)
}

fn virtual_index(u: &Ulid, kind: u64) -> Option<u64> {
    if u.timestamp_ms() == kind {
        Some(u.random() as u64)
    } else {
        None
    }
}

/// The conflict-free replicated grid state.
///
/// Cells are keyed by the **stable** `(column id, row id)` ULID pair they were
/// set against, not by integer `(col, row)` coordinates. This is what lets
/// inserts/deletes shift cell content: the content travels with the row/column
/// ULID, and the integer coordinate is derived from the current canonical
/// ordering. The canonical row/column ordering is rebuilt deterministically from
/// the full set of structural ops (sorted by causal precedence), so concurrent
/// structural edits converge to the same ordering on every peer.
#[derive(Debug, Clone, Default)]
pub struct CrdtStore {
    /// Materialized cell values keyed by `(column id, row id)`.
    cells: HashMap<(Ulid, Ulid), CellValue>,
    /// The winning op precedence for each cell (for LWW).
    cell_clock: HashMap<(Ulid, Ulid), (u64, ActorId)>,
    /// Inserted rows, keyed by id, with their `after` pointer and precedence.
    row_inserts: BTreeMap<Ulid, (Option<Ulid>, (u64, ActorId))>,
    /// Deleted row ids.
    row_deletes: BTreeSet<Ulid>,
    /// Inserted columns, keyed by id, with their `after` pointer and precedence.
    col_inserts: BTreeMap<Ulid, (Option<Ulid>, (u64, ActorId))>,
    /// Deleted column ids.
    col_deletes: BTreeSet<Ulid>,
    /// Materialized row ordering (immutable ids). Virtual rows follow this list.
    rows: Vec<Ulid>,
    /// Materialized column ordering (immutable ids). Virtual cols follow it.
    columns: Vec<Ulid>,
    /// The merged clock seen so far.
    clock: VectorClock,
    /// Local actor id.
    actor: ActorId,
}

impl CrdtStore {
    /// Create a store authored by `actor`.
    pub fn new(actor: ActorId) -> Self {
        CrdtStore {
            actor,
            ..Default::default()
        }
    }

    /// The local actor id.
    pub fn actor(&self) -> ActorId {
        self.actor
    }

    /// Current merged vector clock.
    pub fn clock(&self) -> &VectorClock {
        &self.clock
    }

    /// Author a `SetCell` op for `cell` and apply it locally.
    pub fn set_cell(&mut self, cell: CellId, value: CellValue) -> Op {
        let clock = self.clock.tick(self.actor);
        let op = Op::SetCell {
            cell,
            value,
            clock,
            actor: self.actor,
        };
        self.apply(op.clone());
        op
    }

    /// Author a `DeleteCell` op and apply it locally.
    pub fn delete_cell(&mut self, cell: CellId) -> Op {
        let clock = self.clock.tick(self.actor);
        let op = Op::DeleteCell {
            cell,
            clock,
            actor: self.actor,
        };
        self.apply(op.clone());
        op
    }

    /// Author an `InsertRow` op with a fresh ULID and apply it locally.
    pub fn insert_row(&mut self, after: Option<Ulid>) -> (Ulid, Op) {
        let id = Ulid::new();
        let clock = self.clock.tick(self.actor);
        let op = Op::InsertRow {
            id,
            after,
            clock,
            actor: self.actor,
        };
        self.apply(op.clone());
        (id, op)
    }

    /// Rebuild a deterministic ordering from the set of inserts (sorted by causal
    /// precedence, then id) woven into the virtual baseline, then prune deletes.
    /// Because the input set and the sort are order-independent, every peer that
    /// has seen the same ops rebuilds the identical ordering.
    fn rebuild(
        inserts: &BTreeMap<Ulid, (Option<Ulid>, (u64, ActorId))>,
        deletes: &BTreeSet<Ulid>,
    ) -> Vec<Ulid> {
        let mut items: Vec<(Ulid, Option<Ulid>, (u64, ActorId))> = inserts
            .iter()
            .map(|(&id, &(after, prec))| (id, after, prec))
            .collect();
        // Deterministic causal ordering: higher clock total wins, actor then id
        // break remaining ties so concurrent inserts never depend on arrival.
        items.sort_by(|a, b| a.2.cmp(&b.2).then(a.0.cmp(&b.0)));
        let mut list: Vec<Ulid> = Vec::new();
        for (id, after, _) in items {
            let pos = match after {
                None => 0,
                Some(a) => match list.iter().position(|&x| x == a) {
                    Some(i) => i + 1,
                    None => list.len(),
                },
            };
            list.insert(pos.min(list.len()), id);
        }
        for &d in deletes {
            list.retain(|&x| x != d);
        }
        list
    }

    /// Map an integer row coordinate to its row id (real if within the materialized
    /// ordering, otherwise the deterministic virtual id at that offset).
    fn row_ulid(&self, row: u64) -> Ulid {
        if (row as usize) < self.rows.len() {
            self.rows[row as usize]
        } else {
            virtual_ulid(KIND_ROW, row - self.rows.len() as u64)
        }
    }

    /// Map an integer column coordinate to its column id.
    fn col_ulid(&self, col: u64) -> Ulid {
        if (col as usize) < self.columns.len() {
            self.columns[col as usize]
        } else {
            virtual_ulid(KIND_COL, col - self.columns.len() as u64)
        }
    }

    /// The `(column id, row id)` key a `CellId` resolves to under the current
    /// canonical ordering.
    fn ulid_key(&self, cell: CellId) -> (Ulid, Ulid) {
        let (c, r) = cell.to_rc();
        (self.col_ulid(c), self.row_ulid(r))
    }

    /// Integer coordinate that currently displays a given row/column id.
    fn row_index(&self, ru: Ulid) -> u64 {
        if let Some(p) = self.rows.iter().position(|&x| x == ru) {
            p as u64
        } else if let Some(k) = virtual_index(&ru, KIND_ROW) {
            self.rows.len() as u64 + k
        } else {
            0
        }
    }

    fn col_index(&self, cu: Ulid) -> u64 {
        if let Some(p) = self.columns.iter().position(|&x| x == cu) {
            p as u64
        } else if let Some(k) = virtual_index(&cu, KIND_COL) {
            self.columns.len() as u64 + k
        } else {
            0
        }
    }

    /// Apply a single op, resolving conflicts deterministically.
    pub fn apply(&mut self, op: Op) {
        self.clock.merge(match &op {
            Op::SetCell { clock, .. }
            | Op::DeleteCell { clock, .. }
            | Op::InsertRow { clock, .. }
            | Op::InsertColumn { clock, .. }
            | Op::DeleteRow { clock, .. }
            | Op::DeleteColumn { clock, .. } => clock,
        });
        let prec = precedence(&op);
        match op {
            Op::SetCell { cell, value, .. } => {
                let key = self.ulid_key(cell);
                if self.cell_clock.get(&key).map_or(true, |c| prec >= *c) {
                    self.cells.insert(key, value);
                    self.cell_clock.insert(key, prec);
                }
            }
            Op::DeleteCell { cell, .. } => {
                let key = self.ulid_key(cell);
                if self.cell_clock.get(&key).map_or(true, |c| prec >= *c) {
                    self.cells.remove(&key);
                    self.cell_clock.insert(key, prec);
                }
            }
            Op::InsertRow { id, after, clock, actor } => {
                // Use the op's own causal timestamp (not local state) so that
                // concurrent structural edits resolve identically on every peer.
                let prec = (clock.total(), actor);
                self.row_inserts.insert(id, (after, prec));
                self.rebuild_layout();
            }
            Op::InsertColumn { id, after, clock, actor } => {
                let prec = (clock.total(), actor);
                self.col_inserts.insert(id, (after, prec));
                self.rebuild_layout();
            }
            Op::DeleteRow { id, .. } => {
                self.row_deletes.insert(id);
                self.rebuild_layout();
                self.prune_deleted();
            }
            Op::DeleteColumn { id, .. } => {
                self.col_deletes.insert(id);
                self.rebuild_layout();
                self.prune_deleted();
            }
        }
    }

    fn rebuild_layout(&mut self) {
        self.rows = Self::rebuild(&self.row_inserts, &self.row_deletes);
        self.columns = Self::rebuild(&self.col_inserts, &self.col_deletes);
    }

    /// Drop cells that lived at a deleted row/column so deletion cascades.
    fn prune_deleted(&mut self) {
        self.cells
            .retain(|(cu, ru), _| !self.col_deletes.contains(cu) && !self.row_deletes.contains(ru));
        self.cell_clock
            .retain(|(cu, ru), _| !self.col_deletes.contains(cu) && !self.row_deletes.contains(ru));
    }

    /// Merge another peer's op log into this store. Because application is
    /// order-independent (deterministic LWW), we simply apply every op.
    pub fn merge_ops<I: IntoIterator<Item = Op>>(&mut self, ops: I) {
        for op in ops {
            self.apply(op);
        }
    }

    /// Read a materialized cell value at integer coordinate `id`, resolving it
    /// through the current canonical row/column ordering.
    pub fn get_cell(&self, id: CellId) -> CellValue {
        let key = self.ulid_key(id);
        self.cells.get(&key).cloned().unwrap_or(CellValue::Empty)
    }

    /// Number of materialized rows.
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// Number of materialized columns.
    pub fn column_count(&self) -> usize {
        self.columns.len()
    }

    /// Enumerate every materialized `(CellId, CellValue)` pair, mapping each
    /// stable `(column id, row id)` key back to its current integer coordinate.
    /// Unlike a bounded coordinate rescan, this returns exactly the populated
    /// cells regardless of how large their coordinates are.
    pub fn iter_cells(&self) -> Vec<(CellId, CellValue)> {
        self.cells
            .iter()
            .map(|(&(cu, ru), v)| {
                let c = self.col_index(cu);
                let r = self.row_index(ru);
                (CellId::from_rc(c, r), v.clone())
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vector_clock_ordering() {
        let mut a = VectorClock::new();
        a.tick(1);
        let mut b = a.clone();
        b.tick(2);
        assert!(a.happens_before(&b));
        assert!(!b.happens_before(&a));
    }

    #[test]
    fn concurrent_clocks() {
        let mut a = VectorClock::new();
        a.tick(1);
        let mut b = VectorClock::new();
        b.tick(2);
        assert!(a.concurrent(&b));
    }

    #[test]
    fn offline_edits_converge() {
        // Two peers start empty, go offline, edit different cells, then sync.
        let mut peer_a = CrdtStore::new(1);
        let mut peer_b = CrdtStore::new(2);

        let ops_a = [
            peer_a.set_cell(CellId::from_a1("A1"), CellValue::Number(10.0)),
            peer_a.set_cell(CellId::from_a1("A2"), CellValue::Number(20.0)),
        ];
        let ops_b = [
            peer_b.set_cell(CellId::from_a1("B1"), CellValue::Text("x".into())),
            peer_b.set_cell(CellId::from_a1("A1"), CellValue::Number(999.0)), // concurrent edit on A1
        ];

        // Exchange logs (order-independent).
        peer_a.merge_ops(ops_b.iter().cloned());
        peer_b.merge_ops(ops_a.iter().cloned());

        // Both must converge to identical state.
        assert_eq!(
            peer_a.get_cell(CellId::from_a1("A1")),
            peer_b.get_cell(CellId::from_a1("A1"))
        );
        assert_eq!(
            peer_a.get_cell(CellId::from_a1("A2")),
            peer_b.get_cell(CellId::from_a1("A2"))
        );
        assert_eq!(
            peer_a.get_cell(CellId::from_a1("B1")),
            peer_b.get_cell(CellId::from_a1("B1"))
        );
        // A1 conflict resolves identically: higher actor (2) wins deterministically.
        assert_eq!(
            peer_a.get_cell(CellId::from_a1("A1")),
            CellValue::Number(999.0)
        );
    }

    #[test]
    fn delete_wins_after_set() {
        let mut s = CrdtStore::new(1);
        s.set_cell(CellId::from_a1("A1"), CellValue::Number(1.0));
        s.delete_cell(CellId::from_a1("A1"));
        assert_eq!(s.get_cell(CellId::from_a1("A1")), CellValue::Empty);
    }

    #[test]
    fn row_insertion_uses_ulid() {
        let mut s = CrdtStore::new(1);
        let (r1, _) = s.insert_row(None);
        let (r2, _) = s.insert_row(Some(r1));
        assert_eq!(s.row_count(), 2);
        assert_eq!(s.rows[0], r1);
        assert_eq!(s.rows[1], r2);
    }

    #[test]
    fn insert_row_shifts_existing_content_down() {
        let mut s = CrdtStore::new(1);
        s.set_cell(CellId::from_a1("A1"), CellValue::Number(5.0));
        s.set_cell(CellId::from_a1("A2"), CellValue::Number(6.0));

        // Inserting a row at the top must push existing content down by one.
        s.insert_row(None);

        assert_eq!(s.get_cell(CellId::from_a1("A1")), CellValue::Empty);
        assert_eq!(s.get_cell(CellId::from_a1("A2")), CellValue::Number(5.0));
        assert_eq!(s.get_cell(CellId::from_a1("A3")), CellValue::Number(6.0));
    }

    #[test]
    fn delete_row_cascades_and_shifts_back_up() {
        let mut s = CrdtStore::new(1);
        s.set_cell(CellId::from_a1("A1"), CellValue::Number(5.0));
        s.set_cell(CellId::from_a1("A2"), CellValue::Number(6.0));
        let (r, _) = s.insert_row(None);

        // The inserted row now sits on top; original content is below it.
        assert_eq!(s.get_cell(CellId::from_a1("A1")), CellValue::Empty);
        assert_eq!(s.get_cell(CellId::from_a1("A2")), CellValue::Number(5.0));

        // Deleting the inserted row must drop its (empty) content and shift the
        // rest back up, cascading the delete to any cells that lived there.
        s.apply(Op::DeleteRow {
            id: r,
            clock: s.clock().clone(),
            actor: s.actor(),
        });
        assert_eq!(s.get_cell(CellId::from_a1("A1")), CellValue::Number(5.0));
        assert_eq!(s.get_cell(CellId::from_a1("A2")), CellValue::Number(6.0));
    }

    #[test]
    fn concurrent_structural_ops_converge() {
        // Two peers each insert their own row, then exchange logs. Because the
        // canonical ordering is rebuilt deterministically from the op set (not
        // arrival order), both peers must end up with the identical layout.
        let mut peer_a = CrdtStore::new(1);
        let mut peer_b = CrdtStore::new(2);

        let ops_a = {
            let mut v = Vec::new();
            let (_, ins) = peer_a.insert_row(None);
            v.push(ins);
            v.push(peer_a.set_cell(CellId::from_a1("A1"), CellValue::Number(1.0)));
            v
        };
        let ops_b = {
            let mut v = Vec::new();
            let (_, ins) = peer_b.insert_row(None);
            v.push(ins);
            v.push(peer_b.set_cell(CellId::from_a1("A1"), CellValue::Number(2.0)));
            v
        };

        peer_a.merge_ops(ops_b.iter().cloned());
        peer_b.merge_ops(ops_a.iter().cloned());

        assert_eq!(peer_a.rows, peer_b.rows);
        assert_eq!(peer_a.columns, peer_b.columns);
        // Both peers resolve A1 to the same cell after the layout converges.
        assert_eq!(
            peer_a.get_cell(CellId::from_a1("A1")),
            peer_b.get_cell(CellId::from_a1("A1"))
        );
    }
}
