//! Directed dependency graph (DAG) with cycle detection.
//!
//! Edge direction convention: an edge `D -> C` means *`C` depends on `D`*. This
//! makes a topological ordering over the graph compute dependencies before the
//! cells that depend on them.

use std::collections::{HashMap, HashSet};

use petgraph::algo::{tarjan_scc, toposort};
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use petgraph::Direction;
use tpt_lattice_core::CellId;

/// The maximum number of cells a single `RANGE(...)` may span before it is
/// rejected, to avoid accidentally materializing enormous dependency sets.
pub const MAX_RANGE_CELLS: usize = 1_000_000;

/// Tracks `who I need` (dependencies) and `who needs me` (dependents).
#[derive(Debug, Clone, Default)]
pub struct DependencyGraph {
    graph: DiGraph<CellId, ()>,
    nodes: HashMap<CellId, NodeIndex>,
}

impl DependencyGraph {
    /// Create an empty graph.
    pub fn new() -> Self {
        DependencyGraph::default()
    }

    fn node(&mut self, id: CellId) -> NodeIndex {
        *self
            .nodes
            .entry(id)
            .or_insert_with(|| self.graph.add_node(id))
    }

    /// Replace `cell`'s dependency edges with the supplied set.
    pub fn set_dependencies(&mut self, cell: CellId, deps: &[CellId]) {
        let c = self.node(cell);
        // Remove existing incoming edges (dependency -> cell).
        let incoming: Vec<NodeIndex> = self
            .graph
            .edges_directed(c, Direction::Incoming)
            .map(|e| e.source())
            .collect();
        for s in incoming {
            if let Some(e) = self.graph.find_edge(s, c) {
                self.graph.remove_edge(e);
            }
        }
        let mut seen = HashSet::new();
        for d in deps {
            if !seen.insert(*d) {
                continue;
            }
            let dn = self.node(*d);
            self.graph.add_edge(dn, c, ());
        }
    }

    /// Remove a cell (and all its edges) from the graph.
    pub fn remove(&mut self, cell: CellId) {
        if let Some(n) = self.nodes.remove(&cell) {
            self.graph.remove_node(n);
        }
    }

    /// Cells that `cell` depends on (its predecessors).
    pub fn dependencies(&self, cell: CellId) -> Vec<CellId> {
        match self.nodes.get(&cell) {
            Some(&n) => self
                .graph
                .edges_directed(n, Direction::Incoming)
                .map(|e| self.graph[e.source()])
                .collect(),
            None => Vec::new(),
        }
    }

    /// Cells that depend on `cell` (its successors).
    pub fn dependents(&self, cell: CellId) -> Vec<CellId> {
        match self.nodes.get(&cell) {
            Some(&n) => self
                .graph
                .edges_directed(n, Direction::Outgoing)
                .map(|e| self.graph[e.target()])
                .collect(),
            None => Vec::new(),
        }
    }

    /// All transitive dependents of `cell` (excluding `cell` itself).
    pub fn transitive_dependents(&self, cell: CellId) -> HashSet<CellId> {
        let mut out = HashSet::new();
        let mut stack = self.dependents(cell);
        while let Some(c) = stack.pop() {
            if out.insert(c) {
                stack.extend(self.dependents(c));
            }
        }
        out
    }

    /// All cells that participate in any dependency cycle (via Tarjan's SCCs
    /// of size > 1 and any self-loops). Used to flag circular references.
    pub fn cycle_cells(&self) -> HashSet<CellId> {
        let mut out = HashSet::new();
        for scc in tarjan_scc(&self.graph) {
            if scc.len() > 1 {
                for &n in &scc {
                    out.insert(self.graph[n]);
                }
            } else {
                let n = scc[0];
                if self
                    .graph
                    .edges_directed(n, Direction::Outgoing)
                    .any(|e| e.target() == n)
                {
                    out.insert(self.graph[n]);
                }
            }
        }
        out
    }

    /// Whether any dependency cycle exists. Returns the member cells of one
    /// cycle (via Tarjan's strongly-connected-components) if so.
    pub fn find_cycle(&self) -> Option<Vec<CellId>> {
        for scc in tarjan_scc(&self.graph) {
            if scc.len() > 1 {
                return Some(scc.iter().map(|&n| self.graph[n]).collect());
            }
            let n = scc[0];
            if self
                .graph
                .edges_directed(n, Direction::Outgoing)
                .any(|e| e.target() == n)
            {
                return Some(vec![self.graph[n]]);
            }
        }
        None
    }

    /// Whether the graph is acyclic.
    pub fn is_acyclic(&self) -> bool {
        self.find_cycle().is_none()
    }

    /// A topological ordering of all cells (dependencies precede dependents).
    /// Returns `Err` containing a node inside a cycle if one exists.
    pub fn topo_order(&self) -> Result<Vec<CellId>, CellId> {
        match toposort(&self.graph, None) {
            Ok(order) => Ok(order.into_iter().map(|n| self.graph[n]).collect()),
            Err(n) => Err(self.graph[n.node_id()]),
        }
    }

    /// Kahn topological order restricted to nodes **not** in `skip` (used to
    /// evaluate everything that is not part of a cycle).
    pub fn topo_order_excluding(&self, skip: &HashSet<CellId>) -> Vec<CellId> {
        let mut indeg: HashMap<NodeIndex, usize> = HashMap::new();
        for n in self.graph.node_indices() {
            if skip.contains(&self.graph[n]) {
                continue;
            }
            indeg.entry(n).or_insert(0);
        }
        for e in self.graph.edge_references() {
            let s = e.source();
            let t = e.target();
            if skip.contains(&self.graph[s]) || skip.contains(&self.graph[t]) {
                continue;
            }
            *indeg.entry(t).or_insert(0) += 1;
        }
        let mut ready: Vec<NodeIndex> = indeg
            .iter()
            .filter(|(_, &d)| d == 0)
            .map(|(&n, _)| n)
            .collect();
        let mut order = Vec::new();
        while let Some(n) = ready.pop() {
            order.push(self.graph[n]);
            for e in self.graph.edges_directed(n, Direction::Outgoing) {
                let t = e.target();
                if skip.contains(&self.graph[t]) {
                    continue;
                }
                let d = indeg.get_mut(&t).unwrap();
                *d -= 1;
                if *d == 0 {
                    ready.push(t);
                }
            }
        }
        order
    }

    /// Number of tracked cells.
    pub fn node_count(&self) -> usize {
        self.graph.node_count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dependencies_and_dependents() {
        let mut g = DependencyGraph::new();
        g.set_dependencies(CellId::from_a1("B1"), &[CellId::from_a1("A1")]);
        g.set_dependencies(CellId::from_a1("C1"), &[CellId::from_a1("B1")]);
        assert_eq!(
            g.dependencies(CellId::from_a1("C1")),
            vec![CellId::from_a1("B1")]
        );
        let deps = g.transitive_dependents(CellId::from_a1("A1"));
        assert!(deps.contains(&CellId::from_a1("B1")));
        assert!(deps.contains(&CellId::from_a1("C1")));
    }

    #[test]
    fn detects_cycle() {
        let mut g = DependencyGraph::new();
        g.set_dependencies(CellId::from_a1("A1"), &[CellId::from_a1("B1")]);
        g.set_dependencies(CellId::from_a1("B1"), &[CellId::from_a1("A1")]);
        assert!(g.find_cycle().is_some());
        assert!(!g.is_acyclic());
    }

    #[test]
    fn acyclic_topo() {
        let mut g = DependencyGraph::new();
        g.set_dependencies(CellId::from_a1("B1"), &[CellId::from_a1("A1")]);
        g.set_dependencies(CellId::from_a1("C1"), &[CellId::from_a1("B1")]);
        let order = g.topo_order().unwrap();
        let pos = |c| order.iter().position(|x| *x == c).unwrap();
        assert!(pos(CellId::from_a1("A1")) < pos(CellId::from_a1("B1")));
        assert!(pos(CellId::from_a1("B1")) < pos(CellId::from_a1("C1")));
    }
}
