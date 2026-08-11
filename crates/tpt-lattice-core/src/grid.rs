//! The read/write interface the evaluator operates against.
//!
//! Higher-level crates provide concrete implementations (an in-memory map in
//! `tpt-lattice-evaluator`, a CRDT-backed store in `tpt-lattice-crdt`, a
//! deserialized snapshot in `tpt-lattice-io`). The evaluator depends only on
//! this trait, never on a concrete storage type.

use crate::{CellId, CellValue};

/// A source of cell values that the evaluator can read and write.
///
/// Implementors may be sparse (returning [`CellValue::Empty`] for any unset
/// coordinate) or dense. Reads are required to be total: requesting any valid
/// [`CellId`] must return a [`CellValue`] (never panic).
pub trait GridState {
    /// Read the value stored at `id`. Unset cells return [`CellValue::Empty`].
    fn get_cell(&self, id: CellId) -> CellValue;

    /// Write `value` at `id`, overwriting any previous content.
    fn set_cell(&mut self, id: CellId, value: CellValue);

    /// Whether `id` currently holds a non-empty value.
    ///
    /// The default implementation reads the cell and checks for emptiness;
    /// implementors with an index may override this with an O(1) lookup.
    fn has_cell(&self, id: CellId) -> bool {
        !self.get_cell(id).is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::collections::BTreeMap;

    struct MockGrid {
        cells: BTreeMap<u64, CellValue>,
    }

    impl GridState for MockGrid {
        fn get_cell(&self, id: CellId) -> CellValue {
            self.cells
                .get(&id.to_bits())
                .cloned()
                .unwrap_or(CellValue::Empty)
        }
        fn set_cell(&mut self, id: CellId, value: CellValue) {
            if value.is_empty() {
                self.cells.remove(&id.to_bits());
            } else {
                self.cells.insert(id.to_bits(), value);
            }
        }
    }

    #[test]
    fn trait_roundtrip() {
        let mut g = MockGrid {
            cells: BTreeMap::new(),
        };
        let id = CellId::from_a1("A1");
        assert!(!g.has_cell(id));
        g.set_cell(id, CellValue::Number(7.0));
        assert!(g.has_cell(id));
        assert_eq!(g.get_cell(id), CellValue::Number(7.0));
        g.set_cell(id, CellValue::Empty);
        assert!(!g.has_cell(id));
    }
}
