use std::{
    collections::{BTreeSet, VecDeque},
    sync::LazyLock,
};

use petgraph::{
    Direction::{Incoming, Outgoing},
    algo::{is_cyclic_directed, tarjan_scc},
    graph::NodeIndex,
    visit::{IntoNodeIdentifiers, NodeFiltered},
};

use super::StatsSink;

mod best_effort;
mod bottom_up;
mod common;
mod component_filter;
mod components;
mod config;
mod cycles;
mod dead_vertices;
mod domination;
mod filtration;
mod interface;
mod max_sat_inc_scc;
mod naive;
mod pipeline;
mod sat_descending;
mod support_frontier;
#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) use filtration::filter;
pub(crate) use filtration::filter_with_roots;
use interface::Vertex;
pub use interface::{
    Assignment, AtomicAlgorithm, BestEffortAlgorithm, BestEffortConfig, Bounds, CycleBlocking,
    FilterMode, MaxSatIncSccConfig, Node, Outcome, PreemptiveCyclesConfig, SatDescendingConfig,
    SolverConfig, TimeoutPolicy,
};
pub(crate) use interface::{Graph, VertexId, View};
pub(crate) use pipeline::solve as solve_pipeline;

impl View {
    pub(crate) fn new(graph: &Graph, enabled: Vec<bool>, extra_groups: Vec<Vec<VertexId>>) -> Self {
        debug_assert_eq!(enabled.len(), graph.graph.node_count());
        let mut view = Self {
            enabled,
            extra_groups,
        };
        view.refresh(graph);
        view
    }

    /// A constructor can only be enabled when all of its inputs are enabled.
    pub(crate) fn refresh(&mut self, graph: &Graph) {
        for node in graph.graph.node_indices() {
            if !graph.graph[node].is_all || !self.enabled[node.index()] {
                continue;
            }
            let blocked = graph
                .graph
                .neighbors(node)
                .any(|input| !self.enabled[input.index()]);
            if blocked {
                self.enabled[node.index()] = false;
            }
        }
    }

    pub(crate) fn all_groups<'a>(
        &'a self,
        graph: &'a Graph,
    ) -> impl Iterator<Item = &'a [VertexId]> {
        graph
            .conflict_groups
            .iter()
            .chain(self.extra_groups.iter())
            .map(Vec::as_slice)
    }
}

/// Vertices reachable from `roots` through enabled nodes.
pub(crate) fn reachable(graph: &Graph, roots: &[VertexId], view: &View) -> Vec<bool> {
    let mut reachable = vec![false; graph.graph.node_count()];
    let mut stack = roots.to_vec();
    while let Some(next) = stack.pop() {
        if !view.enabled[next] || reachable[next] {
            continue;
        }
        reachable[next] = true;
        stack.extend(
            graph
                .graph
                .neighbors(NodeIndex::new(next))
                .map(|idx| idx.index())
                .filter(|&id| view.enabled[id]),
        );
    }
    reachable
}

/// A selected vertex must belong to the enabled domain.
pub(crate) fn assignment_within_enabled(view: &View, assignment: &[bool]) -> bool {
    assignment.len() == view.enabled.len()
        && assignment
            .iter()
            .zip(&view.enabled)
            .all(|(&selected, &enabled)| !selected || enabled)
}

pub(crate) fn assignment_cost(graph: &Graph, view: &View, assignment: &[bool]) -> usize {
    graph
        .graph
        .node_indices()
        .filter(|&ni| view.enabled[ni.index()] && assignment[ni.index()])
        .map(|ni| graph.graph[ni].cost)
        .sum()
}

pub(crate) fn check_constraints(
    graph: &Graph,
    roots: &[VertexId],
    view: &View,
    assignment: &[bool],
) -> bool {
    macro_rules! ensure {
        ($cond:expr) => {
            if !$cond {
                return false;
            }
        };
    }

    ensure!(assignment_within_enabled(view, assignment));

    for &root in roots {
        ensure!(assignment[root]);
    }

    let selected = NodeFiltered::from_fn(&graph.graph, |node| assignment[node.index()]);
    for node in selected.node_identifiers() {
        if graph.graph[node].is_all {
            ensure!(graph.graph.neighbors(node).all(|to| assignment[to.index()]));
        } else {
            ensure!(graph.graph.neighbors(node).any(|to| assignment[to.index()]));
        }
    }

    for group in view.all_groups(graph) {
        ensure!(group.iter().filter(|&&i| assignment[i]).count() <= 1);
    }

    graph.is_acyclic(assignment)
}

/// Record shape metrics of the root-reachable subgraph.
///
/// These describe the solver input, not the solver's search path. Expensive
/// parts (degrees, SCCs, 4-cycles) run only when stats are enabled.
pub(crate) fn record_metrics(
    graph: &Graph,
    roots: &[VertexId],
    view: &View,
    stats: &mut StatsSink<'_>,
) {
    let domain = reachable(graph, roots, view);
    stats.count("graph.nodes", graph.graph.node_count() as u64);
    stats.count("graph.edges", graph.graph.edge_count() as u64);
    stats.count("graph.roots", roots.len() as u64);
    stats.count(
        "graph.used_nodes",
        domain.iter().filter(|&&active| active).count() as u64,
    );
    stats.count("graph.conflict_groups", graph.conflict_groups.len() as u64);
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
    for node in graph.graph.node_indices() {
        if !domain[node.index()] {
            continue;
        }
        let out = graph
            .graph
            .neighbors_directed(node, Outgoing)
            .filter(|next| domain[next.index()])
            .count() as u64;
        let incoming = graph
            .graph
            .neighbors_directed(node, Incoming)
            .filter(|next| domain[next.index()])
            .count() as u64;
        used_edges += out;
        max_out = max_out.max(out);
        max_in = max_in.max(incoming);
        let vertex = &graph.graph[node];
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
    for group in view.all_groups(graph) {
        let members = group.iter().filter(|&&id| domain[id]).count() as u64;
        used_group_members += members;
        max_group = max_group.max(members);
    }
    stats.count("graph.conflict_group_members", used_group_members);
    stats.count("graph.conflict_group_max", max_group);

    let sccs = graph.detect_components(&domain);
    let scc_nodes: u64 = sccs.iter().map(|scc| scc.len() as u64).sum();
    let scc_max = sccs.iter().map(|scc| scc.len() as u64).max().unwrap_or(0);
    let scc_sum_squares: u64 = sccs.iter().map(|scc| (scc.len() * scc.len()) as u64).sum();
    stats.count("graph.sccs", sccs.len() as u64);
    stats.count("graph.scc_nodes", scc_nodes);
    stats.count("graph.scc_max", scc_max);
    stats.count("graph.scc_sum_squares", scc_sum_squares);

    // Cyclomatic number per SCC: independent cycles in the domain.
    let (cycle_rank, cycle_rank_max, _) = graph.cycle_analysis(&domain);
    stats.count("graph.cycle_rank", cycle_rank);
    stats.count("graph.cycle_rank_max", cycle_rank_max);

    stats.count(
        "graph.cycles_4",
        graph.detect_cycles_4(&domain).len() as u64,
    );
}

impl Graph {
    pub(crate) fn node_count(&self) -> usize {
        self.graph.node_count()
    }

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

    pub fn add_conflicting_group(&mut self, group: impl IntoIterator<Item = VertexId>) {
        let group: Vec<_> = group.into_iter().collect();
        assert!(!group.is_empty());
        if group.len() > 1 {
            self.conflict_groups.push(group);
        }
    }

    fn detect_components(&self, active: &[bool]) -> Vec<Vec<VertexId>> {
        let filtered = NodeFiltered::from_fn(&self.graph, |node| active[node.index()]);
        tarjan_scc(&filtered)
            .into_iter()
            .filter(|comp| comp.len() > 1)
            .map(|comp| comp.iter().map(|nx| nx.index()).collect())
            .collect()
    }

    /// Cyclomatic number of the active subgraph, total and per-SCC maximum,
    /// plus the mask of nodes in non-trivial SCCs.
    fn cycle_analysis(&self, active: &[bool]) -> (u64, u64, Vec<bool>) {
        let sccs = self.detect_components(active);
        let mut cyclic = vec![false; self.graph.node_count()];
        let mut component_of = vec![usize::MAX; self.graph.node_count()];
        for (component, scc) in sccs.iter().enumerate() {
            for &node in scc {
                component_of[node] = component;
                cyclic[node] = true;
            }
        }
        let mut total = 0u64;
        let mut max = 0u64;
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
            total += rank;
            max = max.max(rank);
        }
        (total, max, cyclic)
    }

    /// Shortest directed cycle in one strongly connected component.
    ///
    /// BFS from each start reaches the first edge back to the start on the
    /// shortest such cycle; the global minimum over starts is the component's
    /// girth. Only active vertices inside the component are traversed.
    fn shortest_cycle(&self, active: &[bool], component: &[VertexId]) -> Option<Vec<VertexId>> {
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
            let Some(cycle) = self.shortest_cycle_from(start, active, &in_component) else {
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
        active: &[bool],
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
                if !active[next] || !in_component[next] {
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

    fn detect_cycles_4(&self, active: &[bool]) -> Vec<[VertexId; 4]> {
        let mut cycles = Vec::new();
        for a in (0..self.graph.node_count()).filter(|&i| active[i]) {
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
