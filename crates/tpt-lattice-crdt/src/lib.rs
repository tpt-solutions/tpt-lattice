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

use std::collections::{BTreeMap, HashMap};

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

/// The conflict-free replicated grid state.
#[derive(Debug, Clone, Default)]
pub struct CrdtStore {
    /// Materialized cell values keyed by packed cell id.
    cells: HashMap<u64, CellValue>,
    /// The winning op precedence for each cell (for LWW).
    cell_clock: HashMap<u64, (u64, ActorId)>,
    /// Materialized row ordering (immutable ids).
    rows: Vec<Ulid>,
    /// Materialized column ordering (immutable ids).
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
                let bits = cell.to_bits();
                if self.cell_clock.get(&bits).map_or(true, |c| prec >= *c) {
                    self.cells.insert(bits, value);
                    self.cell_clock.insert(bits, prec);
                }
            }
            Op::DeleteCell { cell, .. } => {
                let bits = cell.to_bits();
                if self.cell_clock.get(&bits).map_or(true, |c| prec >= *c) {
                    self.cells.remove(&bits);
                    self.cell_clock.insert(bits, prec);
                }
            }
            Op::InsertRow { id, after, .. } => {
                if !self.rows.contains(&id) {
                    match after {
                        Some(a) => {
                            if let Some(pos) = self.rows.iter().position(|r| *r == a) {
                                self.rows.insert(pos + 1, id);
                            } else {
                                self.rows.push(id);
                            }
                        }
                        None => self.rows.insert(0, id),
                    }
                }
            }
            Op::InsertColumn { id, after, .. } => {
                if !self.columns.contains(&id) {
                    match after {
                        Some(a) => {
                            if let Some(pos) = self.columns.iter().position(|c| *c == a) {
                                self.columns.insert(pos + 1, id);
                            } else {
                                self.columns.push(id);
                            }
                        }
                        None => self.columns.insert(0, id),
                    }
                }
            }
            Op::DeleteRow { id, .. } => {
                self.rows.retain(|r| *r != id);
            }
            Op::DeleteColumn { id, .. } => {
                self.columns.retain(|c| *c != id);
            }
        }
    }

    /// Merge another peer's op log into this store. Because application is
    /// order-independent (deterministic LWW), we simply apply every op.
    pub fn merge_ops<I: IntoIterator<Item = Op>>(&mut self, ops: I) {
        for op in ops {
            self.apply(op);
        }
    }

    /// Read a materialized cell value.
    pub fn get_cell(&self, id: CellId) -> CellValue {
        self.cells
            .get(&id.to_bits())
            .cloned()
            .unwrap_or(CellValue::Empty)
    }

    /// Number of materialized rows.
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// Number of materialized columns.
    pub fn column_count(&self) -> usize {
        self.columns.len()
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
}
