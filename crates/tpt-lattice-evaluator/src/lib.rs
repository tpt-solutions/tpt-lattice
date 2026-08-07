//! # tpt-lattice-evaluator
//!
//! Builds the cell dependency DAG, detects cycles, and incrementally evaluates
//! LES formulas against any [`GridState`].
//!
//! ```
//! use tpt_lattice_core::{CellId, CellValue, GridState};
//! use tpt_lattice_parser::parse;
//! use tpt_lattice_evaluator::Evaluator;
//!
//! let mut grid = Evaluator::new();
//! grid.set_value(CellId::from_a1("A1"), CellValue::Number(21.0));
//! grid.set_formula(CellId::from_a1("B1"), "=A1 * 2").unwrap();
//! grid.evaluate().unwrap();
//! assert_eq!(grid.get_value(CellId::from_a1("B1")), CellValue::Number(42.0));
//! ```

use std::collections::{HashMap, HashSet};

use tpt_lattice_core::{CellId, CellValue, GridState, LatticeError};
use tpt_lattice_parser::ast::{CellRef, Expr, Formula};
use tpt_lattice_parser::parse as parse_formula;

mod dag;
mod eval;
mod grid;

pub use dag::{DependencyGraph, MAX_RANGE_CELLS};
pub use grid::InMemoryGrid;

use eval::eval_expr;

/// The calculation engine: owns computed values, raw formulas, and the
/// dependency DAG. Implements [`GridState`] so it can evaluate against itself.
#[derive(Debug, Clone, Default)]
pub struct Evaluator {
    values: HashMap<CellId, CellValue>,
    formulas: HashMap<CellId, Formula>,
    dag: DependencyGraph,
    dirty: HashSet<CellId>,
}

/// Recursively collect every cell referenced by `expr` into `out`. `RANGE(...)`
/// is expanded to the full rectangle of cells it covers (capped).
fn collect_deps(expr: &Expr, out: &mut Vec<CellId>) -> Result<(), LatticeError> {
    match expr {
        Expr::CellRef(c) => {
            if !out.contains(&c.id) {
                out.push(c.id);
            }
        }
        Expr::Range { start, end } => expand_range(start, end, out)?,
        Expr::Unary { expr, .. } => collect_deps(expr, out)?,
        Expr::Binary { left, right, .. } => {
            collect_deps(left, out)?;
            collect_deps(right, out)?;
        }
        Expr::Call { args, .. } => {
            for a in args {
                collect_deps(a, out)?;
            }
        }
        Expr::Cast { expr, .. } => collect_deps(expr, out)?,
        Expr::Match { scrutinee, arms } => {
            collect_deps(scrutinee, out)?;
            for arm in arms {
                collect_deps(&arm.body, out)?;
            }
        }
        Expr::Literal(_) | Expr::Name(_) => {}
    }
    Ok(())
}

/// Expand a range rectangle into `out` (row-major), capped.
fn expand_range(start: &CellRef, end: &CellRef, out: &mut Vec<CellId>) -> Result<(), LatticeError> {
    let (sc, sr) = start.id.to_rc();
    let (ec, er) = end.id.to_rc();
    let (c0, c1) = (sc.min(ec), sc.max(ec));
    let (r0, r1) = (sr.min(er), sr.max(er));
    let count = (c1 - c0 + 1) as usize * (r1 - r0 + 1) as usize;
    if count > MAX_RANGE_CELLS {
        return Err(LatticeError::argument_error("RANGE spans too many cells"));
    }
    for row in r0..=r1 {
        for col in c0..=c1 {
            out.push(CellId::from_rc(col, row));
        }
    }
    Ok(())
}

impl Evaluator {
    /// Create an empty evaluator.
    pub fn new() -> Self {
        Evaluator::default()
    }

    /// Store a literal (non-formula) value at `id`. Any formula previously
    /// attached to `id` is removed and all dependents are marked dirty.
    pub fn set_value(&mut self, id: CellId, value: CellValue) {
        self.formulas.remove(&id);
        self.dag.set_dependencies(id, &[]);
        self.values.insert(id, value.sanitize());
        self.mark_dirty(id);
    }

    /// Parse and attach a formula (with a leading `=`) to `id`. Returns a parse
    /// error if the string is malformed. Dependents are marked dirty.
    pub fn set_formula(&mut self, id: CellId, src: &str) -> Result<(), LatticeError> {
        let formula = parse_formula(src)?;
        let mut deps = Vec::new();
        collect_deps(&formula.body, &mut deps)?;
        self.dag.set_dependencies(id, &deps);
        self.formulas.insert(id, formula);
        self.mark_dirty(id);
        Ok(())
    }

    /// The raw formula attached to `id`, if any.
    pub fn get_formula(&self, id: CellId) -> Option<&Formula> {
        self.formulas.get(&id)
    }

    /// The current computed value at `id` (may be stale until [`evaluate`]).
    pub fn get_value(&self, id: CellId) -> CellValue {
        self.values.get(&id).cloned().unwrap_or(CellValue::Empty)
    }

    /// Whether `id` currently holds a formula.
    pub fn is_formula(&self, id: CellId) -> bool {
        self.formulas.contains_key(&id)
    }

    /// The dependency graph (read-only access for tooling/inspection).
    pub fn dag(&self) -> &DependencyGraph {
        &self.dag
    }

    fn mark_dirty(&mut self, id: CellId) {
        self.dirty.insert(id);
        for d in self.dag.transitive_dependents(id) {
            self.dirty.insert(d);
        }
    }

    /// Recompute all dirty cells in dependency order. Cells inside a circular
    /// reference are set to [`LatticeError::CircularReference`]; everything else
    /// is evaluated top-down so dependency values are resolved first.
    pub fn evaluate(&mut self) -> Result<(), LatticeError> {
        let cycle_cells = self.dag.cycle_cells();
        let order = self.dag.topo_order_excluding(&cycle_cells);

        let mut env: HashMap<String, CellValue> = HashMap::new();
        for id in order {
            if !self.dirty.contains(&id) {
                continue;
            }
            if let Some(formula) = self.formulas.get(&id) {
                let value = eval_expr(&formula.body, self, &mut env);
                self.values.insert(id, value);
            }
        }
        for id in &cycle_cells {
            if self.dirty.contains(id) {
                self.values.insert(
                    *id,
                    CellValue::Error(LatticeError::CircularReference(id.to_a1())),
                );
            }
        }
        self.dirty.clear();
        Ok(())
    }

    /// Attach or update a formula and immediately recompute it and its
    /// transitive dependents.
    pub fn recompute(&mut self, id: CellId, src: &str) -> Result<(), LatticeError> {
        self.set_formula(id, src)?;
        self.evaluate()
    }
}

impl GridState for Evaluator {
    fn get_cell(&self, id: CellId) -> CellValue {
        self.values.get(&id).cloned().unwrap_or(CellValue::Empty)
    }

    fn set_cell(&mut self, id: CellId, value: CellValue) {
        self.set_value(id, value);
    }

    fn has_cell(&self, id: CellId) -> bool {
        self.values.contains_key(&id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell(s: &str) -> CellId {
        CellId::from_a1(s)
    }

    #[test]
    fn formula_chain() {
        let mut e = Evaluator::new();
        e.set_value(cell("A1"), CellValue::Number(2.0));
        e.set_value(cell("A2"), CellValue::Number(3.0));
        e.set_formula(cell("A3"), "=A1 + A2").unwrap();
        e.set_formula(cell("A4"), "=A3 * 10").unwrap();
        e.evaluate().unwrap();
        assert_eq!(e.get_value(cell("A3")), CellValue::Number(5.0));
        assert_eq!(e.get_value(cell("A4")), CellValue::Number(50.0));
    }

    #[test]
    fn cascading_update() {
        let mut e = Evaluator::new();
        e.set_value(cell("A1"), CellValue::Number(1.0));
        e.set_formula(cell("B1"), "=A1 + 1").unwrap();
        e.set_formula(cell("C1"), "=B1 + 1").unwrap();
        e.evaluate().unwrap();
        assert_eq!(e.get_value(cell("C1")), CellValue::Number(3.0));
        // Change the root and re-evaluate: cascades upward.
        e.set_value(cell("A1"), CellValue::Number(10.0));
        e.evaluate().unwrap();
        assert_eq!(e.get_value(cell("C1")), CellValue::Number(12.0));
    }

    #[test]
    fn circular_reference() {
        let mut e = Evaluator::new();
        e.set_formula(cell("A1"), "=B1 + 1").unwrap();
        e.set_formula(cell("B1"), "=A1 + 1").unwrap();
        e.evaluate().unwrap();
        assert!(e.get_value(cell("A1")).is_error());
        assert!(e.get_value(cell("B1")).is_error());
    }

    #[test]
    fn range_sum() {
        let mut e = Evaluator::new();
        for (c, v) in [("A1", 1.0), ("A2", 2.0), ("A3", 3.0)] {
            e.set_value(cell(c), CellValue::Number(v));
        }
        e.set_formula(cell("B1"), "=SUM(RANGE(A1, A3))").unwrap();
        e.evaluate().unwrap();
        assert_eq!(e.get_value(cell("B1")), CellValue::Number(6.0));
    }

    #[test]
    fn strict_typing_errors() {
        let mut e = Evaluator::new();
        e.set_value(cell("A1"), CellValue::Text("5".into()));
        e.set_formula(cell("B1"), "=A1 + 5").unwrap();
        e.evaluate().unwrap();
        assert!(e.get_value(cell("B1")).is_error());
        // Explicit cast fixes it.
        e.set_formula(cell("B1"), "=NUMBER(A1) + 5").unwrap();
        e.evaluate().unwrap();
        assert_eq!(e.get_value(cell("B1")), CellValue::Number(10.0));
    }

    #[test]
    fn match_error_handling() {
        let mut e = Evaluator::new();
        e.set_value(cell("A1"), CellValue::Number(7.0));
        e.set_formula(cell("B1"), "=MATCH(A1, Ok(v) => v * 2, Err(e) => 0)").unwrap();
        e.evaluate().unwrap();
        assert_eq!(e.get_value(cell("B1")), CellValue::Number(14.0));
    }

    #[test]
    fn division_by_zero() {
        let mut e = Evaluator::new();
        e.set_formula(cell("A1"), "=1 / 0").unwrap();
        e.evaluate().unwrap();
        assert_eq!(e.get_value(cell("A1")), CellValue::Error(LatticeError::DivByZero));
    }

    #[test]
    fn dirty_invalidation() {
        let mut e = Evaluator::new();
        e.set_value(cell("A1"), CellValue::Number(2.0));
        e.set_formula(cell("A2"), "=A1 * 3").unwrap();
        e.evaluate().unwrap();
        assert_eq!(e.get_value(cell("A2")), CellValue::Number(6.0));
        // A1 is no longer dirty; changing it marks A2 dirty again.
        e.set_value(cell("A1"), CellValue::Number(4.0));
        assert!(e.dirty.contains(&cell("A2")));
        e.evaluate().unwrap();
        assert_eq!(e.get_value(cell("A2")), CellValue::Number(12.0));
    }
}
