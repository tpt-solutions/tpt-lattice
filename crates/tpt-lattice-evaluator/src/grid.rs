//! A simple, sparse in-memory [`GridState`] used by tests and the engine.

use std::collections::HashMap;

use tpt_lattice_core::{CellId, CellValue};

/// A sparse grid backed by a `HashMap<CellId, CellValue>`.
///
/// Unset cells return [`CellValue::Empty`] and consume no memory. This is the
/// default backing store for [`crate::Evaluator`]; CRDT-backed and
/// deserialized stores implement the same trait.
#[derive(Debug, Clone, Default)]
pub struct InMemoryGrid {
    cells: HashMap<CellId, CellValue>,
}

impl InMemoryGrid {
    /// Create an empty grid.
    pub fn new() -> Self {
        InMemoryGrid {
            cells: HashMap::new(),
        }
    }

    /// Number of non-empty cells.
    pub fn len(&self) -> usize {
        self.cells.len()
    }

    /// Whether the grid holds no cells.
    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    /// Iterate over the currently stored `(CellId, CellValue)` pairs.
    pub fn iter(&self) -> impl Iterator<Item = (&CellId, &CellValue)> {
        self.cells.iter()
    }
}

impl tpt_lattice_core::GridState for InMemoryGrid {
    fn get_cell(&self, id: CellId) -> CellValue {
        self.cells.get(&id).cloned().unwrap_or(CellValue::Empty)
    }

    fn set_cell(&mut self, id: CellId, value: CellValue) {
        if value.is_empty() {
            self.cells.remove(&id);
        } else {
            self.cells.insert(id, value);
        }
    }

    fn has_cell(&self, id: CellId) -> bool {
        self.cells.contains_key(&id)
    }

    fn iter_cells(&self) -> Vec<(CellId, CellValue)> {
        self.cells.iter().map(|(&id, v)| (id, v.clone())).collect()
    }
}
