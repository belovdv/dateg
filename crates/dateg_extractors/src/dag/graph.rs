use std::{
    collections::{BTreeSet, VecDeque},
    sync::LazyLock,
    time::Instant,
};

use petgraph::{
    Direction::{Incoming, Outgoing},
    algo::{is_cyclic_directed, tarjan_scc},
    graph::{DiGraph, NodeIndex},
    visit::{IntoNodeIdentifiers, NodeFiltered},
};

use super::{ExtractError, StatsSink};

mod best_effort;
mod combinator;
mod common;
mod domination;
mod filtration;
mod max_sat_inc_scc;
mod naive;
mod sat_inc_scc;
#[cfg(test)]
mod tests;

pub use best_effort::{BestEffortAlgorithm, BestEffortConfig};
pub use combinator::{CombinatorConfig, CombinatorConfigBuilder, Fallback, FirstResult};
pub use common::CycleBlocking;
pub use filtration::FilterMode;
pub(crate) use filtration::filter;
pub use max_sat_inc_scc::MaxSatIncSccConfig;
pub use sat_inc_scc::SatIncSccConfig;

/// Extraction graph.
///
/// Node ids and adjacency order follow insertion order; [`Self::add_vertex`]
/// and [`Self::add_edges`] preserve it. Extraction algorithms may rely on this
/// order for deterministic tie-breaking, but must not iterate hash maps or
/// treat wall-clock cutoffs as deterministic. Root order is caller-provided.
#[derive(Default)]
pub(crate) struct Graph {
    graph: DiGraph<Vertex, ()>,
    roots: Vec<VertexId>,
    used: Vec<bool>,
    /// Vertices excluded from the solver. Empty means nothing is disabled.
    disabled: Vec<bool>,
    conflict_groups: Vec<Vec<VertexId>>,
}
pub(crate) type VertexId = usize;
#[derive(Debug)]
struct Vertex {
    is_all: bool,
    cost: usize,
    label: Option<String>,
    /// Input classes consumed by this constructor, if any.
    consumes: Vec<VertexId>,
}

/// Selects the extraction algorithm.
#[derive(Clone, Debug, PartialEq)]
pub enum SolverConfig {
    /// Exhaustive search. Only suitable for small graphs; intended as a test oracle.
    Naive,
    /// Best-effort validity: cost-blind heuristics selected by
    /// [`BestEffortConfig`]. The chain walk returns `Unsolved` when its shape
    /// check fails or the rollback budget is exhausted. Not a decision
    /// procedure; intended as a cheap first stage or fallback.
    BestEffort(BestEffortConfig),
    /// Incremental MaxSAT. After each model, block every SCC in the selected
    /// subgraph and re-solve until the selected subgraph is acyclic.
    MaxSatIncScc(MaxSatIncSccConfig),
    /// Plain SAT with a cost-bound assertion, refined by repeated probes. The
    /// first model has no cost bound and only has to be acyclic; its cost is
    /// the initial upper bound. Probes then narrow the bound between zero and
    /// that cost. When the deadline expires during improvement, the best
    /// acyclic model found so far is returned.
    SatIncScc(SatIncSccConfig),
    /// Tries `first` with a small budget and falls back to `second` with the
    /// remaining budget when the first fails in the configured way.
    Combinator(CombinatorConfig),
}

#[derive(Debug)]
pub(crate) struct Solution {
    pub(crate) assignment: Vec<bool>,
    pub(crate) cost: usize,
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
            consumes: Vec::new(),
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

    /// Record which input classes a constructor consumes.
    pub(crate) fn set_consumes(
        &mut self,
        id: VertexId,
        consumed: impl IntoIterator<Item = VertexId>,
    ) {
        self.graph[NodeIndex::new(id)].consumes = consumed.into_iter().collect();
    }

    /// Exclude vertices from the solver without changing vertex ids.
    pub(crate) fn set_disabled(&mut self, disabled: Vec<bool>) {
        debug_assert!(
            disabled.is_empty() || disabled.len() == self.graph.node_count(),
            "disabled mask must match the graph"
        );
        self.disabled = disabled;
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
                SolverConfig::BestEffort(config) => match config.algorithm {
                    BestEffortAlgorithm::Chain => "chain",
                },
                SolverConfig::MaxSatIncScc(_) => "maxsat.inc_scc",
                SolverConfig::SatIncScc(_) => "sat.inc_scc",
                SolverConfig::Combinator(_) => "combinator",
            },
        );

        let solve_started = Instant::now();
        let assignment = match config {
            SolverConfig::Naive => naive::solve(self, deadline).map(|(_, assignment)| assignment),
            SolverConfig::BestEffort(config) => best_effort::solve(self, config, deadline, stats),
            SolverConfig::MaxSatIncScc(config) => (|| {
                let mut solver =
                    max_sat_inc_scc::MaxSatIncSccExtractor::new(self, config, deadline)?;
                solver.solve(deadline, stats)
            })(),
            SolverConfig::SatIncScc(config) => (|| {
                let mut solver =
                    sat_inc_scc::SatIncSccExtractor::new(self, config, deadline, None)?;
                solver.solve(deadline, stats)
            })(),
            SolverConfig::Combinator(config) => self.solve_combinator(config, deadline, stats),
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
        let disabled = self.disabled.clone();
        self.collect_usage_with(&disabled);
    }

    fn collect_usage_with(&mut self, disabled: &[bool]) {
        let is_disabled = |id: VertexId| disabled.get(id).copied().unwrap_or(false);
        self.used = vec![false; self.graph.node_count()];
        let mut queue = self.roots.clone();
        while let Some(next) = queue.pop() {
            if is_disabled(next) || self.used[next] {
                continue;
            }
            self.used[next] = true;
            let ni = NodeIndex::new(next);
            queue.extend(
                self.graph
                    .neighbors(ni)
                    .map(|idx| idx.index())
                    .filter(|&id| !is_disabled(id)),
            );
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

        // Cyclomatic number per SCC: independent cycles in the used graph.
        let mut component_of = vec![usize::MAX; self.graph.node_count()];
        for (component, scc) in sccs.iter().enumerate() {
            for &node in scc {
                component_of[node] = component;
            }
        }
        let mut cycle_rank = 0u64;
        let mut cycle_rank_max = 0u64;
        for (component, scc) in sccs.iter().enumerate() {
            let internal: u64 = scc
                .iter()
                .map(|&node| {
                    self.graph
                        .neighbors(NodeIndex::new(node))
                        .filter(|next| component_of[next.index()] == component)
                        .count() as u64
                })
                .sum();
            let rank = internal.saturating_sub(scc.len() as u64 - 1);
            cycle_rank += rank;
            cycle_rank_max = cycle_rank_max.max(rank);
        }
        stats.count("graph.cycle_rank", cycle_rank);
        stats.count("graph.cycle_rank_max", cycle_rank_max);

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

    /// Shortest directed cycle in one strongly connected component.
    ///
    /// BFS from each start reaches the first edge back to the start on the
    /// shortest such cycle; the global minimum over starts is the component's
    /// girth. Only selected vertices inside the component are traversed.
    fn shortest_cycle(&self, used: &[bool], component: &[VertexId]) -> Option<Vec<VertexId>> {
        if component.len() < 2 {
            return None;
        }
        let mut in_component = vec![false; self.graph.node_count()];
        for &node in component {
            in_component[node] = true;
        }

        let mut best: Option<Vec<VertexId>> = None;
        for &start in component {
            if best.as_ref().is_some_and(|cycle| cycle.len() == 2) {
                break;
            }
            let Some(cycle) = self.shortest_cycle_from(start, used, &in_component) else {
                continue;
            };
            if best.as_ref().is_none_or(|best| cycle.len() < best.len()) {
                best = Some(cycle);
            }
        }
        best
    }

    fn shortest_cycle_from(
        &self,
        start: VertexId,
        used: &[bool],
        in_component: &[bool],
    ) -> Option<Vec<VertexId>> {
        let mut distance = vec![usize::MAX; self.graph.node_count()];
        let mut parent = vec![usize::MAX; self.graph.node_count()];
        let mut queue = VecDeque::new();
        distance[start] = 0;
        queue.push_back(start);

        while let Some(node) = queue.pop_front() {
            for next in self
                .graph
                .neighbors(NodeIndex::new(node))
                .map(|ni| ni.index())
            {
                if !used[next] || !in_component[next] {
                    continue;
                }
                if next == start {
                    let mut cycle = Vec::with_capacity(distance[node] + 1);
                    let mut current = node;
                    while current != start {
                        cycle.push(current);
                        current = parent[current];
                    }
                    cycle.push(start);
                    cycle.reverse();
                    return Some(cycle);
                }
                if distance[next] == usize::MAX {
                    distance[next] = distance[node] + 1;
                    parent[next] = node;
                    queue.push_back(next);
                }
            }
        }
        None
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
