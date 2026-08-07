//! # tpt-lattice-io
//!
//! Serialization of the native Lattice grid to/from MessagePack and compact
//! JSON. Both formats are round-trippable: serialize → deserialize → assert
//! grid equality.
//!
//! ```
//! use tpt_lattice_core::{CellId, CellValue};
//! use tpt_lattice_io::SerializableGrid;
//!
//! let mut g = SerializableGrid::new();
//! g.set(CellId::from_a1("A1"), CellValue::Number(3.0));
//! let bytes = tpt_lattice_io::to_msgpack(&g).unwrap();
//! let back = tpt_lattice_io::from_msgpack(&bytes).unwrap();
//! assert_eq!(g, back);
//! ```

use std::collections::BTreeMap;

use tpt_lattice_core::{CellId, CellValue};

/// A serializable snapshot of grid cell values (keyed by packed `CellId` bits).
///
/// Formulas are intentionally *not* stored here; the evaluator is the source of
/// truth for formulas, while [`SerializableGrid`] captures computed/materialized
/// values for transport and persistence.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SerializableGrid {
    /// Sorted `(cell id bits, value)` pairs for deterministic serialization.
    cells: BTreeMap<u64, CellValue>,
}

impl SerializableGrid {
    /// Create an empty grid snapshot.
    pub fn new() -> Self {
        SerializableGrid {
            cells: BTreeMap::new(),
        }
    }

    /// Number of stored cells.
    pub fn len(&self) -> usize {
        self.cells.len()
    }

    /// Whether the snapshot is empty.
    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    /// Insert or overwrite a cell value.
    pub fn set(&mut self, id: CellId, value: CellValue) {
        if value.is_empty() {
            self.cells.remove(&id.to_bits());
        } else {
            self.cells.insert(id.to_bits(), value);
        }
    }

    /// Read a cell value (empty if unset).
    pub fn get(&self, id: CellId) -> CellValue {
        self.cells
            .get(&id.to_bits())
            .cloned()
            .unwrap_or(CellValue::Empty)
    }

    /// Iterate over `(CellId, CellValue)` pairs in row-major bit order.
    pub fn iter(&self) -> impl Iterator<Item = (CellId, &CellValue)> {
        self.cells
            .iter()
            .map(|(&bits, v)| (CellId::from_bits(bits), v))
    }

    /// Build a snapshot from anything implementing [`tpt_lattice_core::GridState`].
    pub fn from_grid<G: tpt_lattice_core::GridState>(grid: &G) -> Self {
        let mut out = SerializableGrid::new();
        for id in grid_snapshot(grid) {
            out.set(id, grid.get_cell(id));
        }
        out
    }
}

impl Default for SerializableGrid {
    fn default() -> Self {
        Self::new()
    }
}

impl tpt_lattice_core::GridState for SerializableGrid {
    fn get_cell(&self, id: CellId) -> CellValue {
        self.get(id)
    }
    fn set_cell(&mut self, id: CellId, value: CellValue) {
        self.set(id, value);
    }
}

/// Enumerate the non-empty cells of a grid by probing a bounded coordinate range.
fn grid_snapshot<G: tpt_lattice_core::GridState>(grid: &G) -> Vec<CellId> {
    let mut ids = Vec::new();
    for row in 0..1024u64 {
        for col in 0..1024u64 {
            let id = CellId::from_rc(col, row);
            if grid.has_cell(id) {
                ids.push(id);
            }
        }
    }
    ids
}

/// Serialize a snapshot to MessagePack.
pub fn to_msgpack(grid: &SerializableGrid) -> Result<Vec<u8>, rmp_serde::encode::Error> {
    rmp_serde::to_vec_named(grid)
}

/// Deserialize a snapshot from MessagePack.
pub fn from_msgpack(bytes: &[u8]) -> Result<SerializableGrid, rmp_serde::decode::Error> {
    rmp_serde::from_slice(bytes)
}

/// Serialize a snapshot to compact JSON.
pub fn to_json(grid: &SerializableGrid) -> Result<String, serde_json::Error> {
    serde_json::to_string(grid)
}

/// Deserialize a snapshot from compact JSON.
pub fn from_json(s: &str) -> Result<SerializableGrid, serde_json::Error> {
    serde_json::from_str(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tpt_lattice_core::LatticeError;

    fn sample() -> SerializableGrid {
        let mut g = SerializableGrid::new();
        g.set(CellId::from_a1("A1"), CellValue::Number(1.0));
        g.set(CellId::from_a1("B2"), CellValue::Text("hi".into()));
        g.set(CellId::from_a1("C3"), CellValue::Boolean(true));
        g.set(
            CellId::from_a1("D4"),
            CellValue::Error(LatticeError::DivByZero),
        );
        g
    }

    #[test]
    fn msgpack_roundtrip() {
        let g = sample();
        let bytes = to_msgpack(&g).unwrap();
        let back = from_msgpack(&bytes).unwrap();
        assert_eq!(g, back);
    }

    #[test]
    fn json_roundtrip() {
        let g = sample();
        let s = to_json(&g).unwrap();
        let back = from_json(&s).unwrap();
        assert_eq!(g, back);
    }

    #[test]
    fn empty_cell_dropped() {
        let mut g = SerializableGrid::new();
        g.set(CellId::from_a1("A1"), CellValue::Empty);
        assert!(g.is_empty());
    }
}
