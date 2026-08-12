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
use tpt_lattice_parser::ast::{Expr, Formula};
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
        Expr::Range { start, end } => crate::dag::expand_range(start, end, out)?,
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

impl Evaluator {
    /// Create an empty evaluator.
    pub fn new() -> Self {
        Evaluator::default()
    }

    /// Store a literal (non-formula) value at `id`. Any formula previously
    /// attached to `id` is removed and all dependents are marked dirty.
    pub fn set_value(&mut self, id: CellId, value: CellValue) {
        self.formulas.remove(&id);
        if value.is_empty() {
            // The cell is being cleared: prune it entirely from the DAG so it
            // does not permanently occupy a graph node, and drop its value.
            self.dag.remove(id);
            self.values.remove(&id);
        } else {
            self.dag.set_dependencies(id, &[]);
            self.values.insert(id, value.sanitize());
        }
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

        // Mark cycle members as errors *first* so that dependents which read a
        // cycle member during this pass see the error and propagate it, rather
        // than evaluating against a stale (previously-good) value.
        for id in &cycle_cells {
            if self.dirty.contains(id) {
                self.values.insert(
                    *id,
                    CellValue::Error(LatticeError::CircularReference(id.to_a1())),
                );
            }
        }

        let order = self.dag.topo_order_excluding(&cycle_cells);

        let mut env: HashMap<String, CellValue> = HashMap::new();
        for id in order {
            if !self.dirty.contains(&id) {
                continue;
            }
            if let Some(formula) = self.formulas.get(&id) {
                let value = eval_expr(&formula.body, self, &mut env).sanitize();
                self.values.insert(id, value);
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

    fn iter_cells(&self) -> Vec<(CellId, CellValue)> {
        self.values
            .iter()
            .map(|(&id, v)| (id, v.clone()))
            .collect()
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
    fn dependent_of_cycle_errors_instead_of_stale() {
        // C1 depends on A1 which is part of a cycle. It must surface the
        // circular-reference error rather than evaluating against a stale value.
        let mut e = Evaluator::new();
        e.set_formula(cell("A1"), "=B1 + 1").unwrap();
        e.set_formula(cell("B1"), "=A1 + 1").unwrap();
        e.set_formula(cell("C1"), "=A1 + 1").unwrap();
        e.evaluate().unwrap();
        assert!(e.get_value(cell("A1")).is_error());
        assert!(e.get_value(cell("B1")).is_error());
        assert!(e.get_value(cell("C1")).is_error());
    }

    #[test]
    fn non_finite_results_are_sanitized() {
        // SQRT of a negative number yields NaN; it must be stored as an error
        // rather than leaking a non-finite number into the grid.
        let mut e = Evaluator::new();
        e.set_value(cell("A1"), CellValue::Number(-1.0));
        e.set_formula(cell("B1"), "=SQRT(A1)").unwrap();
        e.evaluate().unwrap();
        assert!(e.get_value(cell("B1")).is_error());

        // A literal that parses to infinity must also be sanitized.
        let mut e2 = Evaluator::new();
        e2.set_formula(cell("A1"), "=1e400").unwrap();
        e2.evaluate().unwrap();
        assert!(e2.get_value(cell("A1")).is_error());
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
        e.set_formula(cell("B1"), "=MATCH(A1, Ok(v) => v * 2, Err(e) => 0)")
            .unwrap();
        e.evaluate().unwrap();
        assert_eq!(e.get_value(cell("B1")), CellValue::Number(14.0));
    }

    #[test]
    fn division_by_zero() {
        let mut e = Evaluator::new();
        e.set_formula(cell("A1"), "=1 / 0").unwrap();
        e.evaluate().unwrap();
        assert_eq!(
            e.get_value(cell("A1")),
            CellValue::Error(LatticeError::DivByZero)
        );
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

#[cfg(test)]
mod phase8_tests {
    use super::*;

    fn cell(s: &str) -> CellId {
        CellId::from_a1(s)
    }

    fn text(s: &str) -> CellValue {
        CellValue::Text(s.to_string())
    }

    /// Build a 3x3 table:
    ///   A1=1   B1="a"  C1=10
    ///   A2=2   B2="b"  C2=20
    ///   A3=3   B3="c"  C3=30
    fn table(e: &mut Evaluator) {
        e.set_value(cell("A1"), CellValue::Number(1.0));
        e.set_value(cell("B1"), text("a"));
        e.set_value(cell("C1"), CellValue::Number(10.0));
        e.set_value(cell("A2"), CellValue::Number(2.0));
        e.set_value(cell("B2"), text("b"));
        e.set_value(cell("C2"), CellValue::Number(20.0));
        e.set_value(cell("A3"), CellValue::Number(3.0));
        e.set_value(cell("B3"), text("c"));
        e.set_value(cell("C3"), CellValue::Number(30.0));
    }

    #[test]
    fn concat_operator() {
        let mut e = Evaluator::new();
        e.set_value(cell("A1"), text("Hello"));
        e.set_value(cell("A2"), text("World"));
        e.set_formula(cell("A3"), "=A1 & \" \" & A2").unwrap();
        e.evaluate().unwrap();
        assert_eq!(e.get_value(cell("A3")), text("Hello World"));
        // numbers coerce to text
        e.set_value(cell("A1"), CellValue::Number(2.0));
        e.set_formula(cell("A3"), "=A1 & A1").unwrap();
        e.evaluate().unwrap();
        assert_eq!(e.get_value(cell("A3")), text("22"));
    }

    #[test]
    fn string_functions() {
        let mut e = Evaluator::new();
        e.set_value(cell("A1"), text("  HeLLo  "));
        e.set_formula(cell("B1"), "=UPPER(A1)").unwrap();
        e.set_formula(cell("B2"), "=LOWER(A1)").unwrap();
        e.set_formula(cell("B3"), "=TRIM(A1)").unwrap();
        e.set_formula(cell("B4"), "=LEN(A1)").unwrap();
        e.evaluate().unwrap();
        assert_eq!(e.get_value(cell("B1")), text("  HELLO  "));
        assert_eq!(e.get_value(cell("B2")), text("  hello  "));
        assert_eq!(e.get_value(cell("B3")), text("HeLLo"));
        assert_eq!(e.get_value(cell("B4")), CellValue::Number(9.0));
    }

    #[test]
    fn left_right_mid() {
        let mut e = Evaluator::new();
        e.set_value(cell("A1"), text("Hello"));
        e.set_formula(cell("B1"), "=LEFT(A1, 2)").unwrap();
        e.set_formula(cell("B2"), "=RIGHT(A1, 2)").unwrap();
        e.set_formula(cell("B3"), "=MID(A1, 2, 3)").unwrap();
        e.set_formula(cell("B4"), "=LEFT(A1)").unwrap();
        e.evaluate().unwrap();
        assert_eq!(e.get_value(cell("B1")), text("He"));
        assert_eq!(e.get_value(cell("B2")), text("lo"));
        assert_eq!(e.get_value(cell("B3")), text("ell"));
        assert_eq!(e.get_value(cell("B4")), text("H"));
    }

    #[test]
    fn find_substitute_replace() {
        let mut e = Evaluator::new();
        e.set_value(cell("A1"), text("Hello World"));
        e.set_formula(cell("B1"), "=FIND(\"World\", A1)").unwrap();
        e.set_formula(cell("B2"), "=SUBSTITUTE(A1, \"World\", \"LES\")").unwrap();
        e.set_formula(cell("B3"), "=REPLACE(A1, 1, 5, \"Hi\")").unwrap();
        e.evaluate().unwrap();
        assert_eq!(e.get_value(cell("B1")), CellValue::Number(7.0));
        assert_eq!(e.get_value(cell("B2")), text("Hello LES"));
        assert_eq!(e.get_value(cell("B3")), text("Hi World"));
    }

    #[test]
    fn find_not_found_is_na() {
        let mut e = Evaluator::new();
        e.set_value(cell("A1"), text("abc"));
        e.set_formula(cell("B1"), "=FIND(\"z\", A1)").unwrap();
        e.evaluate().unwrap();
        assert_eq!(e.get_value(cell("B1")), CellValue::Error(LatticeError::NA));
    }

    #[test]
    fn predicates() {
        let mut e = Evaluator::new();
        e.set_value(cell("A1"), CellValue::Empty);
        e.set_value(cell("A2"), CellValue::Number(1.0));
        e.set_value(cell("A3"), text("x"));
        e.set_value(cell("A4"), CellValue::Error(LatticeError::NA));
        e.set_formula(cell("B1"), "=ISBLANK(A1)").unwrap();
        e.set_formula(cell("B2"), "=ISNUMBER(A2)").unwrap();
        e.set_formula(cell("B3"), "=ISTEXT(A3)").unwrap();
        e.set_formula(cell("B4"), "=ISERROR(A4)").unwrap();
        e.set_formula(cell("B5"), "=ISNA(A4)").unwrap();
        e.evaluate().unwrap();
        assert_eq!(e.get_value(cell("B1")), CellValue::Boolean(true));
        assert_eq!(e.get_value(cell("B2")), CellValue::Boolean(true));
        assert_eq!(e.get_value(cell("B3")), CellValue::Boolean(true));
        assert_eq!(e.get_value(cell("B4")), CellValue::Boolean(true));
        assert_eq!(e.get_value(cell("B5")), CellValue::Boolean(true));
    }

    #[test]
    fn iferror_ifna_two_arg_if() {
        let mut e = Evaluator::new();
        e.set_value(cell("A1"), CellValue::Error(LatticeError::DivByZero));
        e.set_formula(cell("B1"), "=IFERROR(A1, 0)").unwrap();
        e.set_formula(cell("B2"), "=IFNA(A1, 99)").unwrap(); // not NA -> passes error through
        e.set_formula(cell("B3"), "=IFERROR(1/0, -1)").unwrap();
        e.set_formula(cell("B4"), "=IF(1 > 0, \"yes\")").unwrap(); // 2-arg IF
        e.evaluate().unwrap();
        assert_eq!(e.get_value(cell("B1")), CellValue::Number(0.0));
        assert!(e.get_value(cell("B2")).is_error());
        assert_eq!(e.get_value(cell("B3")), CellValue::Number(-1.0));
        assert_eq!(e.get_value(cell("B4")), text("yes"));
    }

    #[test]
    fn conditional_aggregates() {
        let mut e = Evaluator::new();
        for (c, v) in [("A1", 1.0), ("A2", 5.0), ("A3", 10.0), ("A4", 5.0)] {
            e.set_value(cell(c), CellValue::Number(v));
        }
        e.set_value(cell("B1"), text("low"));
        e.set_value(cell("B2"), text("high"));
        e.set_value(cell("B3"), text("high"));
        e.set_value(cell("B4"), text("low"));
        e.set_formula(cell("C1"), "=COUNTIF(RANGE(A1, A4), \">4\")").unwrap();
        e.set_formula(cell("C2"), "=SUMIF(RANGE(A1, A4), \">4\")").unwrap();
        e.set_formula(cell("C3"), "=SUMIF(RANGE(B1, B4), \"high\", RANGE(A1, A4))").unwrap();
        e.set_formula(cell("C4"), "=AVERAGEIF(RANGE(B1, B4), \"high\", RANGE(A1, A4))").unwrap();
        e.set_formula(
            cell("C5"),
            "=SUMIFS(RANGE(A1, A4), RANGE(A1, A4), \">4\", RANGE(B1, B4), \"high\")",
        )
        .unwrap();
        e.evaluate().unwrap();
        assert_eq!(e.get_value(cell("C1")), CellValue::Number(3.0));
        assert_eq!(e.get_value(cell("C2")), CellValue::Number(20.0));
        assert_eq!(e.get_value(cell("C3")), CellValue::Number(15.0));
        assert_eq!(e.get_value(cell("C4")), CellValue::Number(7.5));
        assert_eq!(e.get_value(cell("C5")), CellValue::Number(15.0));
    }

    #[test]
    fn lookups() {
        let mut e = Evaluator::new();
        table(&mut e);
        e.set_formula(cell("D1"), "=VLOOKUP(2, RANGE(A1, C3), 3)").unwrap();
        e.set_formula(cell("D2"), "=VLOOKUP(5, RANGE(A1, C3), 3)").unwrap(); // not found
        e.evaluate().unwrap();
        assert_eq!(e.get_value(cell("D1")), CellValue::Number(20.0));
        assert_eq!(e.get_value(cell("D2")), CellValue::Error(LatticeError::NA));

        let mut e2 = Evaluator::new();
        table(&mut e2);
        e2.set_formula(cell("D1"), "=INDEX(RANGE(A1, C3), 2, 3)").unwrap();
        e2.set_formula(cell("D2"), "=INDEX(RANGE(A1, C3), 2, 2)").unwrap();
        e2.set_formula(cell("D3"), "=XLOOKUP(2, RANGE(A1, A3), RANGE(C1, C3))").unwrap();
        e2.evaluate().unwrap();
        assert_eq!(e2.get_value(cell("D1")), CellValue::Number(20.0));
        assert_eq!(e2.get_value(cell("D2")), text("b"));
        assert_eq!(e2.get_value(cell("D3")), CellValue::Number(20.0));

        // HLOOKUP: header row 1, values row 2
        let mut e3 = Evaluator::new();
        e3.set_value(cell("A1"), CellValue::Number(1.0));
        e3.set_value(cell("B1"), CellValue::Number(2.0));
        e3.set_value(cell("C1"), CellValue::Number(3.0));
        e3.set_value(cell("A2"), CellValue::Number(10.0));
        e3.set_value(cell("B2"), CellValue::Number(20.0));
        e3.set_value(cell("C2"), CellValue::Number(30.0));
        e3.set_formula(cell("D1"), "=HLOOKUP(2, RANGE(A1, C2), 2)").unwrap();
        e3.evaluate().unwrap();
        assert_eq!(e3.get_value(cell("D1")), CellValue::Number(20.0));
    }

    #[test]
    fn statistics() {
        let mut e = Evaluator::new();
        for (c, v) in [("A1", 1.0), ("A2", 2.0), ("A3", 3.0), ("A4", 4.0)] {
            e.set_value(cell(c), CellValue::Number(v));
        }
        e.set_formula(cell("B1"), "=MEDIAN(RANGE(A1, A4))").unwrap();
        e.set_formula(cell("B2"), "=VAR(RANGE(A1, A4))").unwrap();
        e.set_formula(cell("B3"), "=STDEV(RANGE(A1, A4))").unwrap();
        e.set_formula(cell("B4"), "=RANK(3, RANGE(A1, A4))").unwrap();
        e.set_formula(cell("B5"), "=PERCENTILE(RANGE(A1, A4), 0.5)").unwrap();
        e.evaluate().unwrap();
        assert_eq!(e.get_value(cell("B1")), CellValue::Number(2.5));
        // sample variance of 1..4 = ((1-2.5)^2+(2-2.5)^2+(3-2.5)^2+(4-2.5)^2)/3 = 1.6666..
        assert!((e.get_value(cell("B2")).as_number().unwrap() - 5.0 / 3.0).abs() < 1e-9);
        assert!((e.get_value(cell("B3")).as_number().unwrap() - (5.0f64 / 3.0).sqrt()).abs() < 1e-9);
        assert_eq!(e.get_value(cell("B4")), CellValue::Number(2.0));
        assert_eq!(e.get_value(cell("B5")), CellValue::Number(2.5));
    }

    #[test]
    fn mode_and_na() {
        let mut e = Evaluator::new();
        for (c, v) in [("A1", 1.0), ("A2", 2.0), ("A3", 2.0), ("A4", 3.0)] {
            e.set_value(cell(c), CellValue::Number(v));
        }
        e.set_formula(cell("B1"), "=MODE(RANGE(A1, A4))").unwrap();
        e.evaluate().unwrap();
        assert_eq!(e.get_value(cell("B1")), CellValue::Number(2.0));

        let mut e2 = Evaluator::new();
        for (c, v) in [("A1", 1.0), ("A2", 2.0), ("A3", 3.0)] {
            e2.set_value(cell(c), CellValue::Number(v));
        }
        e2.set_formula(cell("B1"), "=MODE(RANGE(A1, A3))").unwrap(); // no duplicate
        e2.evaluate().unwrap();
        assert_eq!(e2.get_value(cell("B1")), CellValue::Error(LatticeError::NA));
    }

    #[test]
    fn range_outside_function_errors_clearly() {
        let mut e = Evaluator::new();
        e.set_formula(cell("A1"), "=RANGE(B1, C2)").unwrap();
        e.evaluate().unwrap();
        assert!(e.get_value(cell("A1")).is_error());
    }
}
