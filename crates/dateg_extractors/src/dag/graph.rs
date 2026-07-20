use petgraph::{
    algo::{is_cyclic_directed, tarjan_scc},
    graph::{DiGraph, NodeIndex},
    visit::{IntoNodeIdentifiers, NodeFiltered},
};

mod solve_max_sat;
mod solve_naive;
#[cfg(test)]
mod tests;

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
}

impl Graph {
    pub fn add_vertex(&mut self, is_all: bool, cost: usize) -> VertexId {
        let v = Vertex { is_all, cost };
        self.graph.add_node(v).index()
    }
    pub fn add_edges(&mut self, from: VertexId, to: impl IntoIterator<Item = VertexId>) {
        for to in to {
            assert_ne!(from, to);
            self.graph
                .update_edge(NodeIndex::new(from), NodeIndex::new(to), ());
        }
    }

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

    pub fn solve(&mut self, solver: SolverConfig) -> Vec<bool> {
        self.collect_usage();
        let r = match solver {
            SolverConfig::MaxSat {} => solve_max_sat::DagExtractor::new(self).solve(),
            SolverConfig::Naive => {
                let (cost, r) = solve_naive::solve(self);
                assert!(cost != usize::MAX);
                r
            }
        };
        assert!(self.check_constraints(&r));
        r
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
}

pub enum SolverConfig {
    MaxSat {},
    #[allow(unused)]
    Naive,
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

    #[cfg(test)]
    fn cost(&self, assignment: &[bool]) -> usize {
        self.graph
            .node_indices()
            .filter(|&ni| assignment[ni.index()])
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
