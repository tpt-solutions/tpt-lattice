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

/// The `format_version` written into every serialized snapshot. Bump this when
/// the on-disk shape of [`SerializableGrid`] changes so older files can be
/// detected and migrated/validated on read.
pub const FORMAT_VERSION: u8 = 1;

fn default_format_version() -> u8 {
    FORMAT_VERSION
}

/// A serializable snapshot of grid cell values (keyed by packed `CellId` bits).
///
/// Persists both computed/materialized values (for transport and persistence) and,
/// alongside them, the source LES formulas keyed by the same cell id. On reload a
/// consumer can re-apply the formulas to recover a live sheet instead of dead
/// values. A `format_version` field supports forward/backward compatibility as
/// [`tpt_lattice_core::CellValue`]/`CellId` evolve.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SerializableGrid {
    /// Schema version of this snapshot. Defaults to [`FORMAT_VERSION`] on read so
    /// files written before this field existed still deserialize.
    #[serde(default = "default_format_version")]
    version: u8,
    /// Sorted `(cell id bits, value)` pairs for deterministic serialization.
    cells: BTreeMap<u64, CellValue>,
    /// Source LES formulas keyed by cell id bits (companion to `cells`). A cell
    /// may have a formula, a value, or both.
    #[serde(default)]
    formulas: BTreeMap<u64, String>,
}

impl SerializableGrid {
    /// Create an empty grid snapshot.
    pub fn new() -> Self {
        SerializableGrid {
            version: FORMAT_VERSION,
            cells: BTreeMap::new(),
            formulas: BTreeMap::new(),
        }
    }

    /// The schema version this snapshot was written with.
    pub fn version(&self) -> u8 {
        self.version
    }

    /// Number of stored cells.
    pub fn len(&self) -> usize {
        self.cells.len()
    }

    /// Whether the snapshot has no cells (formulas still count as content).
    pub fn is_empty(&self) -> bool {
        self.cells.is_empty() && self.formulas.is_empty()
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

    /// Set the LES formula for a cell (companion to [`set`](Self::set)). An empty
    /// string removes any stored formula.
    pub fn set_formula(&mut self, id: CellId, formula: &str) {
        if formula.is_empty() {
            self.formulas.remove(&id.to_bits());
        } else {
            self.formulas.insert(id.to_bits(), formula.to_string());
        }
    }

    /// Read the LES formula for a cell, if one is stored.
    pub fn get_formula(&self, id: CellId) -> Option<&str> {
        self.formulas.get(&id.to_bits()).map(|s| s.as_str())
    }

    /// Iterate over `(CellId, formula)` pairs in row-major bit order.
    pub fn iter_formulas(&self) -> impl Iterator<Item = (CellId, &str)> {
        self.formulas
            .iter()
            .map(|(&bits, f)| (CellId::from_bits(bits), f.as_str()))
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
        for (id, value) in grid.iter_cells() {
            out.set(id, value);
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
    fn iter_cells(&self) -> Vec<(CellId, CellValue)> {
        self.iter().map(|(id, v)| (id, v.clone())).collect()
    }
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

    #[test]
    fn version_is_recorded_and_roundtrips() {
        let g = SerializableGrid::new();
        assert_eq!(g.version(), 1);
        let bytes = to_msgpack(&g).unwrap();
        assert_eq!(from_msgpack(&bytes).unwrap().version(), 1);
    }

    #[test]
    fn formulas_persist_and_roundtrip() {
        let mut g = SerializableGrid::new();
        g.set(CellId::from_a1("A1"), CellValue::Number(21.0));
        g.set_formula(CellId::from_a1("B1"), "=A1 * 2");

        let bytes = to_msgpack(&g).unwrap();
        let back = from_msgpack(&bytes).unwrap();
        assert_eq!(back.get(CellId::from_a1("A1")), CellValue::Number(21.0));
        assert_eq!(back.get_formula(CellId::from_a1("B1")), Some("=A1 * 2"));

        // Formula without a stored value still makes the snapshot non-empty.
        let mut g2 = SerializableGrid::new();
        g2.set_formula(CellId::from_a1("C3"), "=1+1");
        assert!(!g2.is_empty());
    }
}
