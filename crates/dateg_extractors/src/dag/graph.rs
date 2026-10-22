use std::{collections::BTreeSet, sync::LazyLock, time::Instant};

use petgraph::{
    Direction::{Incoming, Outgoing},
    algo::{is_cyclic_directed, tarjan_scc},
    graph::{DiGraph, NodeIndex},
    visit::{IntoNodeIdentifiers, NodeFiltered},
};

use super::{ExtractError, StatsSink};

mod solve_max_sat;
mod solve_naive;
#[cfg(test)]
mod tests;

/// Extraction graph.
///
/// Node ids and adjacency order follow insertion order; [`Self::add_vertex`]
/// and [`Self::add_edges`] preserve it. Extraction algorithms may rely on this
/// order for deterministic tie-breaking, but must not iterate hash maps or
/// treat wall-clock cutoffs as deterministic. Root order is caller-provided.
#[derive(Default)]
pub struct Graph {
    graph: DiGraph<Vertex, ()>,
    roots: Vec<VertexId>,
    used: Vec<bool>,
    conflict_groups: Vec<Vec<VertexId>>,
}
pub type VertexId = usize;
#[derive(Debug)]
struct Vertex {
    is_all: bool,
    cost: usize,
    label: Option<String>,
}

/// Selects the extraction algorithm.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SolverConfig {
    /// Exhaustive search. Only suitable for small graphs; intended as a test oracle.
    Naive,
    /// Incremental MaxSAT. After each model, block every SCC in the selected
    /// subgraph and re-solve until the selected subgraph is acyclic.
    MaxSatIncSccBlocking(MaxSatIncSccBlockingConfig),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MaxSatIncSccBlockingConfig {
    /// Preemptively block all 4-cycles before the first solve.
    pub preemptive_4cycles: bool,
}

impl Default for MaxSatIncSccBlockingConfig {
    fn default() -> Self {
        Self {
            preemptive_4cycles: true,
        }
    }
}

#[derive(Debug)]
pub struct Solution {
    pub assignment: Vec<bool>,
    pub cost: usize,
}

impl Graph {
    pub fn add_vertex(
        &mut self,
        is_all: bool,
        cost: usize,
        label: impl FnOnce() -> Option<String>,
    ) -> VertexId {
        static KEEP_LABEL: LazyLock<bool> = LazyLock::new(|| std::env::var("SMT_DEBUG").is_ok());
        let label = KEEP_LABEL.then(label).flatten();

        let v = Vertex {
            is_all,
            cost,
            label,
        };
        self.graph.add_node(v).index()
    }
    pub fn add_edges(&mut self, from: VertexId, to: impl IntoIterator<Item = VertexId>) {
        for to in to {
            assert_ne!(from, to);
            self.graph
                .update_edge(NodeIndex::new(from), NodeIndex::new(to), ());
        }
    }

    /// Set roots in the given order. With multiple roots, this order is part
    /// of the solver input; callers must provide a deterministic order.
    pub fn set_roots(&mut self, root: impl IntoIterator<Item = VertexId>) {
        self.roots = root.into_iter().collect();
    }

    pub fn add_conflicting_group(&mut self, group: impl IntoIterator<Item = VertexId>) {
        let group: Vec<_> = group.into_iter().collect();
        assert!(!group.is_empty());
        if group.len() > 1 {
            self.conflict_groups.push(group);
        }
    }

    /// Solve the graph.
    ///
    /// Determinism contract for all solver implementations, including ones that
    /// do not use z3: node/edge/root/conflict-group order is deterministic for a
    /// fixed graph construction. The algorithm must still make deterministic
    /// choices itself (tie-breaking, no hash-map iteration, fixed RNG seeds,
    /// deterministic parallel merge/order) and must not treat `deadline` as a
    /// deterministic cutoff; it is wall-clock.
    ///
    /// `stats` is an optional out-param: when present, common graph metrics and
    /// solver-specific counters/timings are recorded.
    pub fn solve_with(
        &mut self,
        config: &SolverConfig,
        deadline: Option<Instant>,
        stats: &mut StatsSink<'_>,
    ) -> Result<Solution, ExtractError> {
        self.collect_usage();
        self.record_metrics(stats);
        stats.text(
            "solver.name",
            match config {
                SolverConfig::Naive => "naive",
                SolverConfig::MaxSatIncSccBlocking(_) => "maxsat.inc_scc",
            },
        );

        let solve_started = Instant::now();
        let assignment = match config {
            SolverConfig::Naive => {
                solve_naive::solve(self, deadline).map(|(_, assignment)| assignment)
            }
            SolverConfig::MaxSatIncSccBlocking(config) => (|| {
                let mut solver = solve_max_sat::DagExtractor::new(self, config, deadline)?;
                solver.solve(deadline, stats)
            })(),
        };

        stats.solver(solve_started.elapsed());
        let assignment = assignment?;
        let cost = self.assignment_cost(&assignment);
        Ok(Solution { assignment, cost })
    }

    pub(super) fn validate(&self, assignment: &[bool]) -> bool {
        self.check_constraints(assignment)
    }

    fn collect_usage(&mut self) {
        self.used = vec![false; self.graph.node_count()];
        let mut queue = self.roots.clone();
        while let Some(next) = queue.pop() {
            if !self.used[next] {
                self.used[next] = true;
                let ni = NodeIndex::new(next);
                queue.extend(self.graph.neighbors(ni).map(|idx| idx.index()));
            }
        }
    }

    /// Record shape metrics of the root-reachable subgraph.
    ///
    /// These describe the solver input, not the solver's search path. Expensive
    /// parts (degrees, SCCs, 4-cycles) run only when stats are enabled.
    fn record_metrics(&self, stats: &mut StatsSink<'_>) {
        stats.count("graph.nodes", self.graph.node_count() as u64);
        stats.count("graph.edges", self.graph.edge_count() as u64);
        stats.count("graph.roots", self.roots.len() as u64);
        stats.count(
            "graph.used_nodes",
            self.used.iter().filter(|&&used| used).count() as u64,
        );
        stats.count("graph.conflict_groups", self.conflict_groups.len() as u64);
        if !stats.enabled() {
            return;
        }

        let mut used_edges = 0u64;
        let mut max_out = 0u64;
        let mut max_in = 0u64;
        let mut and_nodes = 0u64;
        let mut or_nodes = 0u64;
        let mut or_leaves = 0u64;
        let mut max_alternatives = 0u64;
        let mut cost_sum = 0u64;
        let mut cost_max = 0u64;
        let mut cost_nonzero = 0u64;
        let mut costs = BTreeSet::new();
        for node in self.graph.node_indices() {
            if !self.used[node.index()] {
                continue;
            }
            let out = self
                .graph
                .neighbors_directed(node, Outgoing)
                .filter(|next| self.used[next.index()])
                .count() as u64;
            let incoming = self
                .graph
                .neighbors_directed(node, Incoming)
                .filter(|next| self.used[next.index()])
                .count() as u64;
            used_edges += out;
            max_out = max_out.max(out);
            max_in = max_in.max(incoming);
            let vertex = &self.graph[node];
            if vertex.is_all {
                and_nodes += 1;
            } else {
                or_nodes += 1;
                max_alternatives = max_alternatives.max(out);
                if out == 0 {
                    or_leaves += 1;
                }
            }
            cost_sum += vertex.cost as u64;
            cost_max = cost_max.max(vertex.cost as u64);
            if vertex.cost > 0 {
                cost_nonzero += 1;
                costs.insert(vertex.cost);
            }
        }
        stats.count("graph.used_edges", used_edges);
        stats.count("graph.and_nodes", and_nodes);
        stats.count("graph.or_nodes", or_nodes);
        stats.count("graph.or_leaves", or_leaves);
        stats.count("graph.max_out", max_out);
        stats.count("graph.max_in", max_in);
        stats.count("graph.max_alternatives", max_alternatives);
        stats.count("graph.cost_sum", cost_sum);
        stats.count("graph.cost_max", cost_max);
        stats.count("graph.cost_nonzero", cost_nonzero);
        stats.count("graph.cost_distinct", costs.len() as u64);

        let mut used_group_members = 0u64;
        let mut max_group = 0u64;
        for group in &self.conflict_groups {
            let members = group.iter().filter(|&&id| self.used[id]).count() as u64;
            used_group_members += members;
            max_group = max_group.max(members);
        }
        stats.count("graph.conflict_group_members", used_group_members);
        stats.count("graph.conflict_group_max", max_group);

        let sccs = self.detect_components(&self.used);
        let scc_nodes: u64 = sccs.iter().map(|scc| scc.len() as u64).sum();
        let scc_max = sccs.iter().map(|scc| scc.len() as u64).max().unwrap_or(0);
        let scc_sum_squares: u64 = sccs.iter().map(|scc| (scc.len() * scc.len()) as u64).sum();
        stats.count("graph.sccs", sccs.len() as u64);
        stats.count("graph.scc_nodes", scc_nodes);
        stats.count("graph.scc_max", scc_max);
        stats.count("graph.scc_sum_squares", scc_sum_squares);
        stats.count("graph.cycles_4", self.detect_cycles_4().len() as u64);
    }
}

impl Graph {
    fn detect_components(&self, used: &[bool]) -> Vec<Vec<VertexId>> {
        let filtered = NodeFiltered::from_fn(&self.graph, |node| used[node.index()]);
        tarjan_scc(&filtered)
            .into_iter()
            .filter(|comp| comp.len() > 1)
            .map(|comp| comp.iter().map(|nx| nx.index()).collect())
            .collect()
    }

    fn is_acyclic(&self, assignment: &[bool]) -> bool {
        let filtered = NodeFiltered::from_fn(&self.graph, |node| assignment[node.index()]);
        !is_cyclic_directed(&filtered)
    }

    fn assignment_cost(&self, assignment: &[bool]) -> usize {
        self.graph
            .node_indices()
            .filter(|&ni| self.used[ni.index()] && assignment[ni.index()])
            .map(|ni| self.graph[ni].cost)
            .sum()
    }

    fn check_constraints(&self, assignment: &[bool]) -> bool {
        macro_rules! ensure {
            ($cond:expr) => {
                if !$cond {
                    return false;
                }
            };
        }

        for &r in &self.roots {
            ensure!(assignment[r]);
        }

        let filtered = NodeFiltered::from_fn(&self.graph, |node| {
            self.used[node.index()] && assignment[node.index()]
        });
        for node in filtered.node_identifiers() {
            if self.graph[node].is_all {
                ensure!(self.graph.neighbors(node).all(|to| assignment[to.index()]));
            } else {
                ensure!(self.graph.neighbors(node).any(|to| assignment[to.index()]));
            }
        }

        for group in &self.conflict_groups {
            ensure!(group.iter().filter(|&&i| assignment[i]).count() <= 1);
        }

        self.is_acyclic(assignment)
    }

    fn detect_cycles_4(&self) -> Vec<[VertexId; 4]> {
        let mut cycles = Vec::new();
        for a in (0..self.graph.node_count()).filter(|&i| self.used[i]) {
            let a_node = NodeIndex::new(a);
            for b in self
                .graph
                .neighbors(a_node)
                .map(|ni| ni.index())
                .filter(|&b| a < b)
            {
                for c in self
                    .graph
                    .neighbors(NodeIndex::new(b))
                    .map(|ni| ni.index())
                    .filter(|&c| a < c)
                {
                    for d in self.graph.neighbors(NodeIndex::new(c)).map(|ni| ni.index()) {
                        if d != a && self.graph.contains_edge(NodeIndex::new(d), a_node) {
                            cycles.push([a, b, c, d]);
                        }
                    }
                }
            }
        }
        log::debug!("cycles_4 len {}", cycles.len());
        cycles
    }
}
