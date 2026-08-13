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

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use tpt_lattice_core::{CellId, CellValue, GridState, LatticeError};
use tpt_lattice_parser::ast::{Expr, Formula};
use tpt_lattice_parser::parse as parse_formula;

mod dag;
mod eval;
mod grid;

pub use dag::{DependencyGraph, MAX_RANGE_CELLS};
pub use grid::InMemoryGrid;

use eval::eval_expr;

/// A user-provided function invoked by the evaluator during formula
/// evaluation. It receives the already-evaluated arguments and returns a
/// [`CellValue`]. Used to bridge to sandboxed wasm plugins and other host
/// extensions.
///
/// The function must be `Send + Sync` so an `Evaluator` remains safely shared
/// across threads (the wasm engine keeps evaluators behind a mutex; the plugin
/// instance itself is stored out-of-band in a thread-local registry, so the
/// closure only needs to be a plain `Send + Sync` function).
pub type ExternalFn = Box<dyn Fn(&[CellValue]) -> CellValue + Send + Sync>;

/// The calculation engine: owns computed values, raw formulas, and the
/// dependency DAG. Implements [`GridState`] so it can evaluate against itself.
pub struct Evaluator {
    values: HashMap<CellId, CellValue>,
    formulas: HashMap<CellId, Formula>,
    /// The raw, user-authored source for each formula cell (preserved verbatim
    /// so snapshots can be restored exactly, including absolute/`$` markers and
    /// sheet qualifiers that the parsed AST may not round-trip).
    formulas_src: HashMap<CellId, String>,
    dag: DependencyGraph,
    dirty: HashSet<CellId>,
    /// Named ranges / reusable formulas, keyed by name. Each value is a LES
    /// expression (typically a `=`-prefixed formula) evaluated lazily against
    /// this evaluator when referenced by name.
    named_ranges: HashMap<String, String>,
    /// Read-only snapshots of other sheets' cell values, enabling cross-sheet
    /// references such as `Sheet2!A1`. Populated by the host (e.g. the cleanup
    /// engine) before evaluation.
    sheet_views: HashMap<String, HashMap<CellId, CellValue>>,
    /// Registered external (user-defined) functions, keyed by upper-cased name.
    externals: HashMap<String, ExternalFn>,
}

impl Default for Evaluator {
    fn default() -> Self {
        Evaluator {
            values: HashMap::new(),
            formulas: HashMap::new(),
            formulas_src: HashMap::new(),
            dag: DependencyGraph::new(),
            dirty: HashSet::new(),
            named_ranges: HashMap::new(),
            sheet_views: HashMap::new(),
            externals: HashMap::new(),
        }
    }
}

impl Clone for Evaluator {
    /// Clone every cell, formula, and dependency, but start with a fresh set of
    /// external functions. Callers that need plugins available on the clone
    /// (e.g. a forked sheet) re-register them afterwards; this keeps the clone
    /// `Send + Sync` regardless of what the host stored in the originals.
    fn clone(&self) -> Self {
        Evaluator {
            values: self.values.clone(),
            formulas: self.formulas.clone(),
            formulas_src: self.formulas_src.clone(),
            dag: self.dag.clone(),
            dirty: self.dirty.clone(),
            named_ranges: self.named_ranges.clone(),
            sheet_views: self.sheet_views.clone(),
            externals: HashMap::new(),
        }
    }
}

impl Evaluator {
    /// Register (or overwrite) an external function callable from formulas by
    /// its upper-cased name. Unknown-but-registered names resolve here instead
    /// of producing a `#NAME?` error.
    pub fn add_external(&mut self, name: &str, f: ExternalFn) {
        self.externals.insert(name.to_ascii_uppercase(), f);
    }

    /// Remove a previously registered external function.
    pub fn remove_external(&mut self, name: &str) {
        self.externals.remove(&name.to_ascii_uppercase());
    }

    /// The names of all currently registered external functions.
    pub fn list_externals(&self) -> Vec<String> {
        self.externals.keys().cloned().collect()
    }

    /// Capture the sheet's current cells + formulas as a snapshot suitable for
    /// diffing, versioning, and 3-way merging.
    pub fn snapshot(&self) -> SheetSnapshot {
        let mut cells = BTreeMap::new();
        for (id, src) in &self.formulas_src {
            cells.insert(
                *id,
                CellSnapshot {
                    value: self.get_value(*id),
                    formula: Some(src.clone()),
                },
            );
        }
        for (id, v) in &self.values {
            if !self.formulas.contains_key(id) {
                cells.insert(*id, CellSnapshot { value: v.clone(), formula: None });
            }
        }
        SheetSnapshot { cells }
    }
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
        self.formulas_src.remove(&id);
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
        // A named range may depend on this cell, so any change could affect
        // formulas that reference a range. Force a full recompute in that case.
        if !self.named_ranges.is_empty() {
            self.mark_all_dirty();
        }
    }

    /// Parse and attach a formula (with a leading `=`) to `id`. Returns a parse
    /// error if the string is malformed. Dependents are marked dirty.
    pub fn set_formula(&mut self, id: CellId, src: &str) -> Result<(), LatticeError> {
        let formula = parse_formula(src)?;
        let mut deps = Vec::new();
        collect_deps(&formula.body, &mut deps)?;
        self.dag.set_dependencies(id, &deps);
        self.formulas.insert(id, formula);
        self.formulas_src.insert(id, src.to_string());
        self.mark_dirty(id);
        Ok(())
    }

    /// The raw formula attached to `id`, if any.
    pub fn get_formula(&self, id: CellId) -> Option<&Formula> {
        self.formulas.get(&id)
    }

    /// All `(cell, source)` pairs for cells that hold a formula. Used by snapshot
    /// restore so formulas survive a round-trip through the op log.
    pub fn list_formulas(&self) -> Vec<(CellId, String)> {
        self.formulas_src
            .iter()
            .map(|(&id, src)| (id, src.clone()))
            .collect()
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

    /// Define or overwrite a named range / reusable formula. `expr` is a LES
    /// expression (with or without a leading `=`); it is re-evaluated every time
    /// the name is referenced, so it always reflects the current grid.
    pub fn set_named_range(&mut self, name: &str, expr: &str) {
        let normalized = expr.strip_prefix('=').unwrap_or(expr).to_string();
        self.named_ranges.insert(name.to_string(), normalized);
        // The change may affect any formula that references the range.
        self.mark_all_dirty();
    }

    /// Remove a named range. Returns `true` if one was present.
    pub fn clear_named_range(&mut self, name: &str) -> bool {
        self.named_ranges.remove(name).is_some()
    }

    /// The expression currently bound to a named range, if any.
    pub fn get_named_range(&self, name: &str) -> Option<&str> {
        self.named_ranges.get(name).map(|s| s.as_str())
    }

    /// All defined named ranges as `(name, expression)` pairs.
    pub fn list_named_ranges(&self) -> Vec<(String, String)> {
        self.named_ranges
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    /// Register read-only snapshots of other sheets' cell values, enabling
    /// cross-sheet references (`Sheet2!A1`). Pass an empty map to clear.
    pub fn set_sheet_views(&mut self, views: HashMap<String, HashMap<CellId, CellValue>>) {
        self.sheet_views = views;
    }

    fn mark_dirty(&mut self, id: CellId) {
        self.dirty.insert(id);
        for d in self.dag.transitive_dependents(id) {
            self.dirty.insert(d);
        }
    }

    /// Mark every formula cell dirty, forcing a full recompute on the next
    /// [`evaluate`]. Used when named ranges change, since their dependencies are
    /// not tracked by the DAG.
    fn mark_all_dirty(&mut self) {
        for id in self.formulas.keys() {
            self.dirty.insert(*id);
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

    fn get_sheet_cell(&self, sheet: &str, id: CellId) -> Option<CellValue> {
        self.sheet_views
            .get(sheet)
            .and_then(|cells| cells.get(&id).cloned())
    }

    fn get_named(&self, name: &str) -> Option<CellValue> {
        let expr = self.named_ranges.get(name)?;
        let formula = parse_formula(expr).ok()?;
        let mut env: HashMap<String, CellValue> = HashMap::new();
        // Evaluate against this evaluator (as the GridState). A named range that
        // references itself would recurse; callers should avoid that.
        Some(crate::eval::eval_expr(&formula.body, self, &mut env).sanitize())
    }

    fn call_external(&self, name: &str, args: &[CellValue]) -> Option<CellValue> {
        self.externals
            .get(&name.to_ascii_uppercase())
            .map(|f| f(args))
    }
}

// ---------------------------------------------------------------------------
// Snapshots, diffs, and 3-way merges (versioning / branching support)
// ---------------------------------------------------------------------------

/// A single cell's persisted state: its computed value and, if it holds a
/// formula, the raw source. Two cells are "equal" only when both their value
/// and their formula match.
#[derive(Debug, Clone, PartialEq)]
pub struct CellSnapshot {
    pub value: CellValue,
    pub formula: Option<String>,
}

/// A point-in-time capture of a whole sheet, used for diffing and merging.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SheetSnapshot {
    pub cells: BTreeMap<CellId, CellSnapshot>,
}

/// The relationship of a cell between two snapshots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffKind {
    /// Present only in the right-hand snapshot.
    Added,
    /// Present only in the left-hand snapshot.
    Removed,
    /// Present in both but with a different value or formula.
    Changed,
    /// Identical in both snapshots.
    Unchanged,
}

/// One row of a [`diff`] result.
#[derive(Debug, Clone, PartialEq)]
pub struct DiffRow {
    pub cell: CellId,
    pub kind: DiffKind,
    pub left: Option<CellSnapshot>,
    pub right: Option<CellSnapshot>,
}

/// Compute a cell-level diff between two snapshots (left = "before", right =
/// "after"). Cells present in only one side are reported as added/removed; cells
/// differing in value or formula are reported as changed; identical cells as
/// unchanged. Output is ordered by cell coordinate.
pub fn diff(left: &SheetSnapshot, right: &SheetSnapshot) -> Vec<DiffRow> {
    let mut rows = Vec::new();
    let ids: BTreeSet<CellId> = left
        .cells
        .keys()
        .chain(right.cells.keys())
        .copied()
        .collect();
    for id in ids {
        let l = left.cells.get(&id).cloned();
        let r = right.cells.get(&id).cloned();
        let kind = match (&l, &r) {
            (None, Some(_)) => DiffKind::Added,
            (Some(_), None) => DiffKind::Removed,
            (Some(a), Some(b)) if a == b => DiffKind::Unchanged,
            (Some(_), Some(_)) => DiffKind::Changed,
            (None, None) => continue,
        };
        rows.push(DiffRow {
            cell: id,
            kind,
            left: l,
            right: r,
        });
    }
    rows
}

/// A cell where both sides changed differently relative to the base, so it
/// cannot be auto-merged.
#[derive(Debug, Clone, PartialEq)]
pub struct MergeConflict {
    pub cell: CellId,
    pub base: Option<CellSnapshot>,
    pub ours: Option<CellSnapshot>,
    pub theirs: Option<CellSnapshot>,
}

/// The result of a 3-way merge: the (non-conflicting) changes to apply to the
/// "ours" sheet, plus any conflicts that need manual resolution.
#[derive(Debug, Clone, Default)]
pub struct MergeResult {
    pub applied: Vec<(CellId, CellSnapshot)>,
    pub conflicts: Vec<MergeConflict>,
}

/// 3-way merge of `base` into `ours` using `theirs` (the branch). For each cell:
/// - only `theirs` changed relative to `base` → take `theirs` (applied).
/// - only `ours` changed (or both changed identically) → keep `ours` (nothing to
///   apply, since `ours` already holds it).
/// - both changed differently → conflict.
///
/// `applied` entries are written to the `ours` sheet by the caller. A deletion
/// on one side is represented by a [`CellSnapshot`] whose `value` is
/// [`CellValue::Empty`] and whose `formula` is `None`.
pub fn three_way_merge(
    base: &SheetSnapshot,
    ours: &SheetSnapshot,
    theirs: &SheetSnapshot,
) -> MergeResult {
    let mut result = MergeResult::default();
    let ids: BTreeSet<CellId> = base
        .cells
        .keys()
        .chain(ours.cells.keys())
        .chain(theirs.cells.keys())
        .copied()
        .collect();
    for id in ids {
        let b = base.cells.get(&id).cloned();
        let o = ours.cells.get(&id).cloned();
        let t = theirs.cells.get(&id).cloned();
        if o == t {
            // Identical on both sides (including "both unchanged"): nothing to do.
            continue;
        }
        if o == b {
            // Only theirs changed (or ours == base and theirs changed): take theirs.
            if let Some(t) = t {
                result.applied.push((id, t));
            } else {
                // theirs deleted it.
                result.applied.push((
                    id,
                    CellSnapshot {
                        value: CellValue::Empty,
                        formula: None,
                    },
                ));
            }
        } else if t == b {
            // Only ours changed: keep ours (already applied). Nothing to do.
            continue;
        } else {
            // Both diverged differently: conflict.
            result.conflicts.push(MergeConflict {
                cell: id,
                base: b,
                ours: o,
                theirs: t,
            });
        }
    }
    result
}

#[cfg(test)]
mod versioning_tests {
    use super::*;

    fn cell(s: &str) -> CellId {
        CellId::from_a1(s)
    }

    #[test]
    fn snapshot_roundtrips_cell_and_formula() {
        let mut e = Evaluator::new();
        e.set_value(cell("A1"), CellValue::Number(3.0));
        e.set_formula(cell("A2"), "=A1 * 2").unwrap();
        let snap = e.snapshot();
        assert_eq!(snap.cells.get(&cell("A1")).unwrap().value, CellValue::Number(3.0));
        assert_eq!(snap.cells.get(&cell("A1")).unwrap().formula, None);
        assert_eq!(snap.cells.get(&cell("A2")).unwrap().formula.as_deref(), Some("=A1 * 2"));
    }

    #[test]
    fn diff_detects_added_removed_changed() {
        let mut a = Evaluator::new();
        a.set_value(cell("A1"), CellValue::Number(1.0));
        a.set_value(cell("B1"), CellValue::Number(2.0));
        a.set_formula(cell("C1"), "=A1").unwrap();
        let snap_a = a.snapshot();

        let mut b = Evaluator::new();
        b.set_value(cell("A1"), CellValue::Number(1.0)); // unchanged
        b.set_value(cell("B1"), CellValue::Number(9.0)); // changed
        // C1 removed
        b.set_value(cell("D1"), CellValue::Number(4.0)); // added
        let snap_b = b.snapshot();

        let rows = diff(&snap_a, &snap_b);
        let by_cell: std::collections::HashMap<CellId, DiffKind> =
            rows.iter().map(|r| (r.cell, r.kind)).collect();
        assert_eq!(by_cell[&cell("A1")], DiffKind::Unchanged);
        assert_eq!(by_cell[&cell("B1")], DiffKind::Changed);
        assert_eq!(by_cell[&cell("C1")], DiffKind::Removed);
        assert_eq!(by_cell[&cell("D1")], DiffKind::Added);
    }

    #[test]
    fn three_way_merge_takes_theirs_when_only_branch_changed() {
        let mut base = Evaluator::new();
        base.set_value(cell("A1"), CellValue::Number(1.0));
        base.set_value(cell("A2"), CellValue::Number(2.0));
        let b = base.snapshot();

        let ours = b.clone();
        let mut theirs = b.clone();
        theirs.cells.insert(cell("A1"), CellSnapshot { value: CellValue::Number(10.0), formula: None });
        theirs.cells.remove(&cell("A2"));
        theirs.cells.insert(cell("A3"), CellSnapshot { value: CellValue::Number(3.0), formula: None });

        let r = three_way_merge(&b, &ours, &theirs);
        assert!(r.conflicts.is_empty());
        let applied: std::collections::HashMap<CellId, CellSnapshot> = r.applied.into_iter().collect();
        assert_eq!(applied[&cell("A1")].value, CellValue::Number(10.0));
        assert_eq!(applied[&cell("A3")].value, CellValue::Number(3.0));
        assert_eq!(applied[&cell("A2")].value, CellValue::Empty);
    }

    #[test]
    fn three_way_merge_reports_conflict_when_both_change() {
        let mut base = Evaluator::new();
        base.set_value(cell("A1"), CellValue::Number(1.0));
        let b = base.snapshot();

        let mut ours = b.clone();
        ours.cells.insert(cell("A1"), CellSnapshot { value: CellValue::Number(2.0), formula: None });
        let mut theirs = b.clone();
        theirs.cells.insert(cell("A1"), CellSnapshot { value: CellValue::Number(3.0), formula: None });

        let r = three_way_merge(&b, &ours, &theirs);
        assert_eq!(r.conflicts.len(), 1);
        assert_eq!(r.conflicts[0].cell, cell("A1"));
        assert!(r.applied.is_empty());
    }

    #[test]
    fn external_function_is_called_from_formula() {
        let mut e = Evaluator::new();
        e.set_value(cell("A1"), CellValue::Number(21.0));
        e.add_external("DOUBLE", Box::new(|args: &[CellValue]| {
            match args.first() {
                Some(CellValue::Number(n)) => CellValue::Number(n * 2.0),
                _ => CellValue::Error(LatticeError::type_error("Number", "other")),
            }
        }));
        e.set_formula(cell("B1"), "=DOUBLE(A1)").unwrap();
        e.evaluate().unwrap();
        assert_eq!(e.get_value(cell("B1")), CellValue::Number(42.0));
    }

    #[test]
    fn unknown_external_function_is_name_error() {
        let mut e = Evaluator::new();
        e.set_formula(cell("A1"), "=NOSUCH(1)").unwrap();
        e.evaluate().unwrap();
        assert!(e.get_value(cell("A1")).is_error());
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

#[cfg(test)]
mod date_tests {
    use super::*;

    fn cell(s: &str) -> CellId {
        CellId::from_a1(s)
    }

    #[test]
    fn date_constructor_and_parts() {
        let mut e = Evaluator::new();
        // 2020-03-15 -> serial 43905
        e.set_formula(cell("A1"), "=DATE(2020, 3, 15)").unwrap();
        e.evaluate().unwrap();
        assert_eq!(
            e.get_value(cell("A1")),
            CellValue::Date(tpt_lattice_core::serial_from_ymd(2020, 3, 15))
        );
        e.set_formula(cell("A2"), "=YEAR(A1)").unwrap();
        e.set_formula(cell("A3"), "=MONTH(A1)").unwrap();
        e.set_formula(cell("A4"), "=DAY(A1)").unwrap();
        e.evaluate().unwrap();
        assert_eq!(e.get_value(cell("A2")), CellValue::Number(2020.0));
        assert_eq!(e.get_value(cell("A3")), CellValue::Number(3.0));
        assert_eq!(e.get_value(cell("A4")), CellValue::Number(15.0));
    }

    #[test]
    fn datedif_units() {
        let mut e = Evaluator::new();
        e.set_value(cell("A1"), CellValue::Date(tpt_lattice_core::serial_from_ymd(2019, 1, 1)));
        e.set_value(cell("A2"), CellValue::Date(tpt_lattice_core::serial_from_ymd(2021, 3, 15)));
        e.set_formula(cell("B1"), "=DATEDIF(A1, A2, \"Y\")").unwrap();
        e.set_formula(cell("B2"), "=DATEDIF(A1, A2, \"M\")").unwrap();
        e.set_formula(cell("B3"), "=DATEDIF(A1, A2, \"D\")").unwrap();
        e.set_formula(cell("B4"), "=DATEDIF(A1, A2, \"YM\")").unwrap();
        e.set_formula(cell("B5"), "=DATEDIF(A1, A2, \"MD\")").unwrap();
        e.evaluate().unwrap();
        assert_eq!(e.get_value(cell("B1")), CellValue::Number(2.0)); // 2 full years
        assert_eq!(e.get_value(cell("B2")), CellValue::Number(26.0)); // 26 months
        assert_eq!(e.get_value(cell("B3")), CellValue::Number(804.0)); // 804 days
        assert_eq!(e.get_value(cell("B4")), CellValue::Number(2.0)); // 2 months within year
        assert_eq!(e.get_value(cell("B5")), CellValue::Number(14.0)); // 14 days
    }

    #[test]
    fn today_and_now_return_dates() {
        let mut e = Evaluator::new();
        e.set_formula(cell("A1"), "=TODAY()").unwrap();
        e.set_formula(cell("A2"), "=NOW()").unwrap();
        e.evaluate().unwrap();
        assert!(matches!(e.get_value(cell("A1")), CellValue::Date(_)));
        assert!(matches!(e.get_value(cell("A2")), CellValue::Date(_)));
    }

    #[test]
    fn date_type_error_on_non_date() {
        let mut e = Evaluator::new();
        e.set_value(cell("A1"), CellValue::Text("x".into()));
        e.set_formula(cell("A2"), "=YEAR(A1)").unwrap();
        e.evaluate().unwrap();
        assert!(e.get_value(cell("A2")).is_error());
    }
}

#[cfg(test)]
mod scope_tests {
    use super::*;

    fn cell(s: &str) -> CellId {
        CellId::from_a1(s)
    }

    #[test]
    fn named_range_reusable_formula() {
        let mut e = Evaluator::new();
        e.set_value(cell("A1"), CellValue::Number(10.0));
        e.set_value(cell("A2"), CellValue::Number(5.0));
        // A named range that references live cells.
        e.set_named_range("TaxRate", "=A2 / 100");
        e.set_formula(cell("B1"), "=A1 * TaxRate").unwrap();
        e.evaluate().unwrap();
        // 10 * (5/100) = 0.5
        assert_eq!(e.get_value(cell("B1")), CellValue::Number(0.5));
        // Re-defining the underlying cell updates the named range result.
        e.set_value(cell("A2"), CellValue::Number(20.0));
        e.evaluate().unwrap();
        assert_eq!(e.get_value(cell("B1")), CellValue::Number(2.0));
        assert!(e.get_named_range("TaxRate").is_some());
        assert_eq!(e.list_named_ranges().len(), 1);
    }

    #[test]
    fn named_range_unknown_is_name_error() {
        let mut e = Evaluator::new();
        e.set_formula(cell("A1"), "=Nope + 1").unwrap();
        e.evaluate().unwrap();
        assert!(e.get_value(cell("A1")).is_error());
    }

    #[test]
    fn cross_sheet_reference() {
        let mut e = Evaluator::new();
        e.set_value(cell("A1"), CellValue::Number(42.0));
        // Build a view of another sheet containing its own A1 = 7.
        let mut other = HashMap::new();
        other.insert(cell("A1"), CellValue::Number(7.0));
        let mut views = HashMap::new();
        views.insert("Sheet2".to_string(), other);
        e.set_sheet_views(views);
        e.set_formula(cell("B1"), "=Sheet2!A1 * 2").unwrap();
        e.evaluate().unwrap();
        assert_eq!(e.get_value(cell("B1")), CellValue::Number(14.0));
    }

    #[test]
    fn cross_sheet_unknown_sheet_is_ref_error() {
        let mut e = Evaluator::new();
        e.set_formula(cell("A1"), "=Ghost!A1").unwrap();
        e.evaluate().unwrap();
        assert!(e.get_value(cell("A1")).is_error());
    }
}
