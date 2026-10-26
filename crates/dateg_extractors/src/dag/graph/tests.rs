use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
use std::time::Instant;

use petgraph::dot::{Config, Dot};
use rand::{
    Rng, SeedableRng,
    seq::{IndexedRandom, SliceRandom},
};
use rayon::iter::{IntoParallelIterator, ParallelIterator};

use crate::dag::{Error, ExtractStats, StatValue, StatsSink};

use super::*;

#[derive(Debug, PartialEq)]
struct Solution {
    assignment: Vec<bool>,
    cost: usize,
}

fn solver(pipeline: Node) -> SolverConfig {
    SolverConfig {
        pipeline,
        timeout: None,
        soft_timeout: None,
        name: None,
    }
}

fn maxsat() -> SolverConfig {
    solver(Node::Atomic(AtomicAlgorithm::MaxSat(Default::default())))
}

fn sat() -> SolverConfig {
    solver(Node::Atomic(AtomicAlgorithm::SatDescending(
        Default::default(),
    )))
}

fn best_effort() -> SolverConfig {
    solver(Node::Atomic(AtomicAlgorithm::Chain(
        BestEffortConfig::default(),
    )))
}

fn maxsat_scc() -> SolverConfig {
    solver(Node::Atomic(AtomicAlgorithm::MaxSat(MaxSatIncSccConfig {
        cycle_blocking: CycleBlocking::Scc,
        ..Default::default()
    })))
}

fn sat_scc() -> SolverConfig {
    solver(Node::Atomic(AtomicAlgorithm::SatDescending(
        SatDescendingConfig {
            cycle_blocking: CycleBlocking::Scc,
            ..Default::default()
        },
    )))
}

#[derive(Default)]
struct TestGraph {
    inner: Graph,
    roots: Vec<VertexId>,
    disabled: Vec<bool>,
}

impl std::ops::Deref for TestGraph {
    type Target = Graph;

    fn deref(&self) -> &Graph {
        &self.inner
    }
}

impl std::ops::DerefMut for TestGraph {
    fn deref_mut(&mut self) -> &mut Graph {
        &mut self.inner
    }
}

impl TestGraph {
    fn set_roots(&mut self, roots: impl IntoIterator<Item = VertexId>) {
        self.roots = roots.into_iter().collect();
    }

    fn set_disabled(&mut self, disabled: Vec<bool>) {
        debug_assert!(
            disabled.is_empty() || disabled.len() == self.inner.graph.node_count(),
            "disabled mask must match the graph"
        );
        self.disabled = disabled;
    }

    fn enabled(&self) -> Vec<bool> {
        let count = self.inner.graph.node_count();
        if self.disabled.is_empty() {
            vec![true; count]
        } else {
            self.disabled.iter().map(|&disabled| !disabled).collect()
        }
    }

    fn view(&self) -> View {
        View::new(&self.inner, self.enabled(), Vec::new())
    }

    fn solve_with(
        &mut self,
        config: &SolverConfig,
        deadline: Option<Instant>,
        stats: &mut StatsSink<'_>,
    ) -> Result<Solution, Error> {
        let outcome = super::pipeline::solve(
            &self.inner,
            &self.roots,
            self.enabled(),
            config,
            deadline,
            None,
            stats,
        )?;
        match outcome {
            Outcome::Optimal(assignment) | Outcome::Any { assignment, .. } => {
                let view = self.view();
                Ok(Solution {
                    cost: assignment_cost(&self.inner, &view, &assignment.selected),
                    assignment: assignment.selected,
                })
            }
            Outcome::Infeasible => Err(Error::Infeasible),
        }
    }

    fn check_constraints(&self, assignment: &[bool]) -> bool {
        check_constraints(&self.inner, &self.roots, &self.view(), assignment)
    }

    fn assignment_cost(&self, assignment: &[bool]) -> usize {
        assignment_cost(&self.inner, &self.view(), assignment)
    }

    fn validate(&self, assignment: &[bool]) -> bool {
        check_constraints(&self.inner, &self.roots, &self.view(), assignment)
    }
}

fn solve(g: &mut TestGraph) -> Solution {
    g.solve_with(&maxsat(), None, &mut StatsSink(None)).unwrap()
}

fn solve_sat_with(g: &mut TestGraph, config: &SolverConfig) -> Solution {
    g.solve_with(config, None, &mut StatsSink(None)).unwrap()
}

fn solve_sat(g: &mut TestGraph) -> Solution {
    solve_sat_with(g, &sat())
}

/// Cost of the selected subgraph reachable from the roots. The `Sat` solver
/// must match this, not just `assignment_cost`: a model may otherwise select
/// extra disconnected nodes and over-report its cost.
fn selected_reachable_cost(g: &TestGraph, assignment: &[bool]) -> usize {
    let mut seen = vec![false; g.graph.node_count()];
    let mut stack = g.roots.clone();
    let mut cost = 0;
    while let Some(next) = stack.pop() {
        if seen[next] {
            continue;
        }
        seen[next] = true;
        let node = NodeIndex::new(next);
        cost += g.graph[node].cost;
        stack.extend(
            g.graph
                .neighbors(node)
                .filter(|&next| assignment[next.index()])
                .map(|next| next.index()),
        );
    }
    cost
}

fn solve_naive(g: &mut TestGraph) -> Result<Solution, Error> {
    g.solve_with(
        &solver(Node::Atomic(AtomicAlgorithm::Naive)),
        None,
        &mut StatsSink(None),
    )
}

impl Error {
    pub(crate) fn panic_with_graph(self, g: &Graph, assignment: &[bool]) -> ! {
        // Graph dumps are only useful for the tiny generated cases.
        if g.graph.node_count() <= 32 {
            dbg!(&g.graph);
            let dot = Dot::with_config(&g.graph, &[Config::EdgeNoLabel, Config::NodeIndexLabel]);
            eprintln!("{dot:?}");
            eprintln!("{assignment:?}");
        }
        panic!("{self}")
    }
}

#[test]
fn acyclic_subgraph_extraction() {
    let mut g = TestGraph::default();
    let root = g.add_vertex(true, 1, || None);
    let a = g.add_vertex(true, 1, || None);
    let b = g.add_vertex(false, 1, || None);
    let c = g.add_vertex(true, 1, || None);
    let d = g.add_vertex(true, 1, || None);
    g.set_roots([root]);
    g.add_edges(root, [a, b]);
    g.add_edges(b, [c, d]);
    let assignment = solve(&mut g).assignment;
    assert!(
        vec![true, true, true, false, true] == assignment
            || vec![true, true, true, true, false] == assignment
    );

    let mut g = TestGraph::default();
    let root = g.add_vertex(true, 0, || None);
    let a = g.add_vertex(false, 1, || None);
    let b = g.add_vertex(false, 1, || None);
    let c = g.add_vertex(false, 1, || None);
    let d = g.add_vertex(true, 1, || None);
    let e = g.add_vertex(true, 1, || None);
    g.set_roots([root]);
    g.add_edges(root, [a, c]);
    g.add_edges(a, [b, d]);
    g.add_edges(b, [a, c]);
    g.add_edges(c, [b, e]);
    let assignment = solve(&mut g).assignment;
    assert_eq!(assignment, vec![true, true, false, true, true, true]);

    let mut g = TestGraph::default();
    let root = g.add_vertex(true, 0, || None);
    let x = g.add_vertex(false, 100, || None);
    let y = g.add_vertex(true, 0, || None);
    g.set_roots([root]);
    g.add_edges(x, [y]);
    let assignment = solve(&mut g).assignment;
    assert_eq!(assignment, vec![true, false, false]);

    let mut g = TestGraph::default();
    let root = g.add_vertex(false, 0, || None);
    let a = g.add_vertex(true, 1, || None);
    let b = g.add_vertex(true, 0, || None);
    let c = g.add_vertex(true, 0, || None);
    g.set_roots([root]);
    g.add_edges(root, [a, b, c]);
    g.add_conflicting_group([a, b, c]);
    let solution = solve(&mut g).assignment;
    assert!(
        solution == vec![true, false, true, false] || solution == vec![true, false, false, true]
    );

    let num = 300;
    let count_fail_to_build = AtomicUsize::new(0);
    (0..num).into_par_iter().for_each(|i| {
        let mut rng = rand::rngs::StdRng::seed_from_u64(i as u64);

        let n = rng.random_range(9..=12);
        let mut g = TestGraph::default();

        let mut assignment = vec![false; n];
        for v in 0..n {
            assignment[v] = rng.random_bool(0.5);
        }
        let num_true = assignment.iter().filter(|&&b| b).count();
        if num_true < 4 || num_true > n - 3 {
            count_fail_to_build.fetch_add(1, SeqCst);
            return;
        }

        let true_vertices: Vec<_> = (0..n).filter(|&v| assignment[v]).collect();
        let false_vertices: Vec<_> = (0..n).filter(|&v| !assignment[v]).collect();

        let root_count = rng.random_range(1..=true_vertices.len().min(2));
        let roots = {
            let mut tv = true_vertices.clone();
            tv.shuffle(&mut rng);
            tv[..root_count].to_vec()
        };
        g.set_roots(roots);

        let mut vertices = vec![];
        for _ in 0..n {
            let is_all = rng.random_bool(0.5);
            let cost = rng.random_range(0..=5usize);
            let id = g.add_vertex(is_all, cost, || None);
            vertices.push(id);
        }

        let mut order: Vec<usize> = (0..n).collect();
        order.shuffle(&mut rng);
        for &from in &vertices {
            let is_all = g.graph.node_weight(NodeIndex::new(from)).unwrap().is_all;
            let mut candidates: Vec<usize> = true_vertices
                .iter()
                .filter(|&&i| order[i] > order[from])
                .copied()
                .collect();
            candidates.shuffle(&mut rng);
            let tos = if assignment[from] && is_all {
                if candidates.is_empty() {
                    continue;
                }
                candidates[..rng.random_range(0..candidates.len())].to_vec()
            } else if assignment[from] {
                assert!(!is_all);
                if candidates.is_empty() {
                    count_fail_to_build.fetch_add(1, SeqCst);
                    return;
                }
                let num = if candidates.len() > 1 {
                    rng.random_range(1..candidates.len())
                } else {
                    1
                };
                candidates[..num]
                    .iter()
                    .chain(false_vertices.iter().filter(|_| rng.random_bool(0.2)))
                    .copied()
                    .filter(|&to| to != from)
                    .collect()
            } else {
                let mut r: Vec<_> = (0..n)
                    .filter(|_| rng.random_bool(0.25))
                    .filter(|&to| to != from)
                    .collect();
                while !is_all && r.is_empty() {
                    let id = rng.random_range(0..n);
                    if id != from {
                        r.push(id);
                    }
                }
                r
            };
            g.add_edges(from, tos);
        }

        let n_groups = rng.random_range(0..=2);
        for _ in 0..n_groups {
            let mut false_vertices = false_vertices.clone();
            false_vertices.shuffle(&mut rng);
            let num = rng.random_range(0..false_vertices.len());
            let mut group = false_vertices[..num].to_vec();
            if rng.random_bool(0.5) {
                group.push(*true_vertices.choose(&mut rng).unwrap());
            }
            if group.len() >= 1 {
                g.add_conflicting_group(group);
            }
        }

        let naive = match solve_naive(&mut g) {
            Ok(solution) => solution,
            Err(error) => error.panic_with_graph(&g, &assignment),
        };
        let maxsat = solve(&mut g);

        assert!(g.check_constraints(&maxsat.assignment), "test {i}: invalid");

        assert_eq!(
            naive.cost, maxsat.cost,
            "test {}: {} != {}",
            i, naive.cost, maxsat.cost
        );

        let sat = solve_sat(&mut g);
        assert!(g.check_constraints(&sat.assignment), "test {i}: invalid");
        assert_eq!(naive.cost, sat.cost, "test {i}: naive vs sat");
        assert_eq!(
            sat.cost,
            selected_reachable_cost(&g, &sat.assignment),
            "test {i}: sat cost must match the selected subgraph"
        );

        let maxsat_scc = solve_sat_with(&mut g, &maxsat_scc());
        assert_eq!(naive.cost, maxsat_scc.cost, "test {i}: naive vs maxsat_scc");

        let sat_scc = solve_sat_with(&mut g, &sat_scc());
        assert_eq!(naive.cost, sat_scc.cost, "test {i}: naive vs sat_scc");

        // The best-effort search commits a choice once its own requirements are
        // met, so it is a fast heuristic rather than a decision procedure: a
        // returned assignment must be valid and no better than the optimum, and
        // a rare `Unsolved` is tolerated (the rate is asserted below).
        match g.solve_with(&best_effort(), None, &mut StatsSink(None)) {
            Ok(best_effort) => {
                assert!(
                    g.check_constraints(&best_effort.assignment),
                    "test {i}: best-effort invalid"
                );
                assert!(
                    best_effort.cost >= naive.cost,
                    "test {i}: best-effort cost {} below optimum {}",
                    best_effort.cost,
                    naive.cost
                );
            }
            Err(Error::NotFound) => {}
            Err(error) => error.panic_with_graph(&g, &assignment),
        }
    });
    let failed = count_fail_to_build.load(SeqCst);
    assert!(num - failed > 100, "only {} succeeded", num - failed);
}

#[test]
fn solver_config_and_stats() {
    let mut g = TestGraph::default();
    let root = g.add_vertex(true, 0, || None);
    let a = g.add_vertex(true, 1, || None);
    let b = g.add_vertex(true, 2, || None);
    g.set_roots([root]);
    g.add_edges(root, [a, b]);

    let mut stats = ExtractStats::default();
    let solved = {
        let mut sink = StatsSink(Some(&mut stats));
        g.solve_with(&maxsat(), None, &mut sink).unwrap()
    };
    assert_eq!(solved.cost, 3);
    assert_eq!(solved.cost, g.assignment_cost(&solved.assignment));
    assert_eq!(stats.custom["graph.nodes"], StatValue::Count(3));
    assert_eq!(stats.custom["graph.edges"], StatValue::Count(2));
    assert_eq!(stats.custom["graph.used_nodes"], StatValue::Count(3));
    assert_eq!(stats.custom["graph.conflict_groups"], StatValue::Count(0));
    assert!(stats.custom.contains_key("maxsat.inc_scc.steps"));
    assert!(stats.solver.is_some());
    assert!(stats.verify.is_none());

    let naive = solve_naive(&mut g).unwrap();
    assert_eq!(naive.cost, solved.cost);

    let failure = g
        .solve_with(&maxsat(), Some(Instant::now()), &mut StatsSink(None))
        .unwrap_err();
    assert!(matches!(failure, Error::Timeout));

    let mut g = TestGraph::default();
    let root = g.add_vertex(true, 0, || None);
    let leaf = g.add_vertex(false, 1, || None);
    g.set_roots([root]);
    g.add_edges(root, [leaf]);
    let failure = solve_naive(&mut g).unwrap_err();
    assert!(matches!(failure, Error::Infeasible));
}

#[test]
fn sat_zero_cost() {
    let mut g = TestGraph::default();
    let root = g.add_vertex(false, 0, || None);
    let leaf = g.add_vertex(true, 0, || None);
    g.set_roots([root]);
    g.add_edges(root, [leaf]);

    let mut stats = ExtractStats::default();
    let solved = {
        let mut sink = StatsSink(Some(&mut stats));
        g.solve_with(&sat(), None, &mut sink).unwrap()
    };
    assert_eq!(solved.cost, 0);
    assert_eq!(stats.custom["sat.descending.probes"], StatValue::Count(0));
    assert_eq!(stats.custom["sat.descending.optimal"], StatValue::Count(1));
}

/// A stateful chain: `init -> store1 -> store2`, with pure constants. The
/// chain solver must walk from the base to the root and materialize the cone.
#[test]
fn chain_walks_from_the_base_to_the_root() {
    let mut g = TestGraph::default();
    let m0 = g.add_vertex(false, 0, || None);
    let m1 = g.add_vertex(false, 0, || None);
    let m2 = g.add_vertex(false, 0, || None);
    let init = g.add_vertex(true, 1, || None);
    let store1 = g.add_vertex(true, 1, || None);
    let store2 = g.add_vertex(true, 1, || None);
    let c0 = g.add_vertex(false, 0, || None);
    let cst0 = g.add_vertex(true, 1, || None);
    let c1 = g.add_vertex(false, 0, || None);
    let cst1 = g.add_vertex(true, 1, || None);
    g.set_roots([m2]);
    g.add_edges(m0, [init]);
    g.add_edges(m1, [store1]);
    g.add_edges(m2, [store2]);
    g.add_edges(c0, [cst0]);
    g.add_edges(c1, [cst1]);
    g.add_edges(store1, [m0, c0]);
    g.add_edges(store2, [m1, c1]);
    g.set_consumes(store1, [m0]);
    g.set_consumes(store2, [m1]);

    let mut stats = ExtractStats::default();
    let solution = {
        let mut sink = StatsSink(Some(&mut stats));
        g.solve_with(&best_effort(), None, &mut sink).unwrap()
    };
    assert!(g.check_constraints(&solution.assignment));
    assert_eq!(stats.custom["solver.name"], StatValue::Text("chain".into()));
    assert_eq!(stats.custom["chain.shape"], StatValue::Text("ok".into()));
    assert_eq!(stats.custom["chain.length"], StatValue::Count(2));
    assert_eq!(solution.cost, 5);

    // Deterministic.
    let again = solve_sat_with(&mut g, &best_effort());
    assert_eq!(solution.assignment, again.assignment);
}

/// A read of a class that is not derivable from the prefix makes the candidate
/// invalid; the solver must roll back and take the next consumer.
#[test]
fn chain_rolls_back_when_a_read_is_not_derivable() {
    let mut g = TestGraph::default();
    let m0 = g.add_vertex(false, 0, || None);
    let m1 = g.add_vertex(false, 0, || None);
    let m2 = g.add_vertex(false, 0, || None);
    let init = g.add_vertex(true, 1, || None);
    let good = g.add_vertex(true, 1, || None);
    let bad = g.add_vertex(true, 0, || None);
    let store2 = g.add_vertex(true, 1, || None);
    let c = g.add_vertex(false, 0, || None);
    let cst = g.add_vertex(true, 1, || None);
    g.set_roots([m2]);
    g.add_edges(m0, [init]);
    g.add_edges(m1, [bad, good]);
    g.add_edges(m2, [store2]);
    g.add_edges(c, [cst]);
    // `bad` reads the root, which cannot be produced before it; it is cheaper,
    // so the greedy order tries it first.
    g.add_edges(bad, [m0, m2]);
    g.add_edges(good, [m0, c]);
    g.add_edges(store2, [m1, c]);
    g.set_consumes(bad, [m0]);
    g.set_consumes(good, [m0]);
    g.set_consumes(store2, [m1]);

    let mut stats = ExtractStats::default();
    let solution = {
        let mut sink = StatsSink(Some(&mut stats));
        g.solve_with(&best_effort(), None, &mut sink).unwrap()
    };
    assert!(g.check_constraints(&solution.assignment));
    assert!(!solution.assignment[bad]);
    assert!(solution.assignment[good]);
    assert!(matches!(
        stats.custom["chain.rollbacks"],
        StatValue::Count(rollbacks) if rollbacks > 0
    ));
}

/// Constructors with two consumable inputs do not have the chain shape; the
/// probe declines instead of guessing.
#[test]
fn chain_reports_unsolved_on_a_non_chain_shape() {
    let mut g = TestGraph::default();
    let a = g.add_vertex(false, 0, || None);
    let b = g.add_vertex(false, 0, || None);
    let root = g.add_vertex(false, 0, || None);
    let init_a = g.add_vertex(true, 1, || None);
    let init_b = g.add_vertex(true, 1, || None);
    let both = g.add_vertex(true, 1, || None);
    g.set_roots([root]);
    g.add_edges(a, [init_a]);
    g.add_edges(b, [init_b]);
    g.add_edges(root, [both]);
    g.add_edges(both, [a, b]);
    g.set_consumes(both, [a, b]);

    let mut stats = ExtractStats::default();
    let failure = {
        let mut sink = StatsSink(Some(&mut stats));
        g.solve_with(&best_effort(), None, &mut sink).unwrap_err()
    };
    assert!(matches!(failure, Error::NotFound));
    assert_eq!(
        stats.custom["chain.shape"],
        StatValue::Text("unsupported".into())
    );
}

/// A cycle alternative (`x_loop`) next to a direct one (`x_direct`). The ids
/// make the cycle the first choice, so a backtracking search must move past it;
/// the pure closure can simply pick the direct alternative.
fn graph_with_a_cycle_alternative() -> TestGraph {
    let mut g = TestGraph::default();
    let root = g.add_vertex(false, 0, || None);
    let via_x = g.add_vertex(true, 1, || None);
    let x = g.add_vertex(false, 0, || None);
    let x_loop = g.add_vertex(true, 5, || None);
    let x_direct = g.add_vertex(true, 1, || None);
    let z = g.add_vertex(false, 0, || None);
    let z_ctor = g.add_vertex(true, 1, || None);
    g.set_roots([root]);
    g.add_edges(root, [via_x]);
    g.add_edges(via_x, [x]);
    g.add_edges(x, [x_loop, x_direct]);
    g.add_edges(x_loop, [root]);
    g.add_edges(x_direct, [z]);
    g.add_edges(z, [z_ctor]);
    g
}

/// The best-effort entry point has no consumers here, so its pure closure
/// chooses around the cyclic alternative; the result must be deterministic.
#[test]
fn best_effort_avoids_a_cycle() {
    let mut g = graph_with_a_cycle_alternative();
    let mut stats = ExtractStats::default();
    let first = {
        let mut sink = StatsSink(Some(&mut stats));
        g.solve_with(&best_effort(), None, &mut sink).unwrap()
    };
    assert!(g.check_constraints(&first.assignment));
    assert_eq!(
        first.assignment,
        vec![true, true, true, false, true, true, true]
    );
    assert_eq!(first.cost, 3);
    assert_eq!(stats.custom["solver.name"], StatValue::Text("chain".into()));

    let second = solve_sat_with(&mut g, &best_effort());
    assert_eq!(first.assignment, second.assignment);
}

/// A class whose only alternative loops back to the root has no acyclic
/// production; the heuristic reports `Unsolved` after exhausting its search.
#[test]
fn best_effort_reports_unsolved() {
    let mut g = TestGraph::default();
    let root = g.add_vertex(false, 0, || None);
    let x = g.add_vertex(false, 0, || None);
    let x_loop = g.add_vertex(true, 1, || None);
    g.set_roots([root]);
    g.add_edges(root, [x]);
    g.add_edges(x, [x_loop]);
    g.add_edges(x_loop, [root]);

    let failure = g
        .solve_with(&best_effort(), None, &mut StatsSink(None))
        .unwrap_err();
    assert!(matches!(failure, Error::NotFound));
}

/// Exercise the new pipeline surface directly: a filter node followed by an
/// atomic solver in a bundle, with the public outcome/assignment types.
#[test]
fn pipeline_bundle_filter_and_atomic() {
    let mut g = TestGraph::default();
    let root = g.add_vertex(false, 0, || None);
    let expensive = g.add_vertex(true, 5, || None);
    let cheap = g.add_vertex(true, 1, || None);
    g.set_roots([root]);
    g.add_edges(root, [expensive, cheap]);

    let pipeline = SolverConfig {
        pipeline: Node::Bundle(vec![
            Node::Filter(FilterMode::AtLeastOneOptimal),
            Node::Atomic(AtomicAlgorithm::MaxSat(Default::default())),
        ]),
        timeout: None,
        soft_timeout: None,
        name: None,
    };
    let enabled = vec![true; g.node_count()];
    let outcome = super::pipeline::solve(
        &g,
        &[root],
        enabled,
        &pipeline,
        None,
        None,
        &mut StatsSink(None),
    )
    .unwrap();
    let Outcome::Optimal(assignment) = outcome else {
        panic!("expected an optimal outcome");
    };
    assert!(g.check_constraints(&assignment.selected));
    assert_eq!(g.assignment_cost(&assignment.selected), 1);
}

/// The same abstract graph has multiple optimal assignments when built in
/// different orders: MaxSAT, the plain SAT baseline, and the non-z3 exhaustive
/// solver pick different ones. Once the construction order is fixed, repeated
/// extractions must agree. This makes the determinism assertion non-vacuous.
#[test]
fn ambiguous_extraction_is_deterministic_by_construction_order() {
    let attrs = [
        (false, 3),
        (false, 0),
        (false, 0),
        (true, 4),
        (true, 0),
        (true, 3),
        (true, 0),
        (false, 0),
        (false, 1),
        (false, 0),
    ];
    let edges = [
        (0, 3),
        (0, 6),
        (0, 1),
        (1, 4),
        (1, 5),
        (1, 7),
        (1, 9),
        (2, 5),
        (4, 3),
        (4, 0),
        (4, 8),
        (7, 4),
        (7, 6),
        (8, 6),
        (8, 1),
        (9, 1),
        (9, 5),
    ];
    let groups: [&[usize]; 1] = [&[2, 3]];

    fn build(
        perm: &[usize],
        attrs: &[(bool, usize)],
        edges: &[(usize, usize)],
        groups: &[&[usize]],
    ) -> TestGraph {
        let mut new_id = vec![0; attrs.len()];
        for (index, &old) in perm.iter().enumerate() {
            new_id[old] = index;
        }
        let mut g = TestGraph::default();
        for &old in perm {
            let (is_all, cost) = attrs[old];
            g.add_vertex(is_all, cost, || None);
        }
        g.set_roots([new_id[0]]);
        for &(from, to) in edges {
            g.add_edges(new_id[from], [new_id[to]]);
        }
        for group in groups {
            g.add_conflicting_group(group.iter().map(|&value| new_id[value]));
        }
        g
    }

    fn run_maxsat(g: &mut TestGraph) -> (usize, Vec<bool>) {
        let solution = g.solve_with(&maxsat(), None, &mut StatsSink(None)).unwrap();
        (solution.cost, solution.assignment)
    }

    fn run_naive(g: &mut TestGraph) -> (usize, Vec<bool>) {
        let solution = solve_naive(g).unwrap();
        (solution.cost, solution.assignment)
    }

    fn run_sat(g: &mut TestGraph) -> (usize, Vec<bool>) {
        let solution = solve_sat(g);
        (solution.cost, solution.assignment)
    }

    let identity: Vec<usize> = (0..attrs.len()).collect();
    let reordered = [1, 6, 4, 5, 3, 2, 9, 7, 8, 0];

    let mut g = build(&identity, &attrs, &edges, &groups);
    let maxsat_identity = run_maxsat(&mut g);
    let mut g = build(&reordered, &attrs, &edges, &groups);
    let maxsat_reordered = run_maxsat(&mut g);

    let mut g = build(&identity, &attrs, &edges, &groups);
    let naive_identity = run_naive(&mut g);
    let mut g = build(&reordered, &attrs, &edges, &groups);
    let naive_reordered = run_naive(&mut g);

    let mut g = build(&identity, &attrs, &edges, &groups);
    let sat_identity = run_sat(&mut g);
    let mut g = build(&reordered, &attrs, &edges, &groups);
    let sat_reordered = run_sat(&mut g);

    assert_eq!(maxsat_identity.0, maxsat_reordered.0);
    assert_ne!(maxsat_identity.1, maxsat_reordered.1);
    assert_eq!(naive_identity.0, naive_reordered.0);
    assert_ne!(naive_identity.1, naive_reordered.1);
    assert_eq!(sat_identity.0, sat_reordered.0);
    assert_ne!(sat_identity.1, sat_reordered.1);

    for _ in 0..4 {
        let mut g = build(&identity, &attrs, &edges, &groups);
        assert_eq!(maxsat_identity, run_maxsat(&mut g));
        let mut g = build(&identity, &attrs, &edges, &groups);
        assert_eq!(naive_identity, run_naive(&mut g));
        let mut g = build(&identity, &attrs, &edges, &groups);
        assert_eq!(sat_identity, run_sat(&mut g));
    }
}

mod filtration {
    use super::*;
    use crate::dag::graph::{FilterMode, filter};

    #[test]
    fn removes_self_referential_alternative() {
        let mut g = TestGraph::default();
        let class = g.add_vertex(false, 0, || None);
        let store = g.add_vertex(true, 1, || None);
        let leaf = g.add_vertex(true, 1, || None);
        g.set_roots([class]);
        g.add_edges(class, [store, leaf]);
        g.add_edges(store, [class]);

        let report = filter(&g, FilterMode::AtLeastOneSolution);
        assert!(report.disabled[store]);
        assert!(!report.disabled[leaf]);
        assert_eq!(report.removed_self, 1);
    }

    #[test]
    fn keeps_consumers() {
        let mut g = TestGraph::default();
        let class = g.add_vertex(false, 0, || None);
        let mem = g.add_vertex(false, 0, || None);
        let store = g.add_vertex(true, 1, || None);
        let pure = g.add_vertex(true, 1, || None);
        g.set_roots([class]);
        g.add_edges(class, [store, pure]);
        g.add_edges(store, [mem]);
        g.set_consumes(store, [mem]);

        let report = filter(&g, FilterMode::AtLeastOneSolution);
        assert!(!report.disabled[store]);
        assert_eq!(report.consumers, 1);
    }

    #[test]
    fn subset_domination_respects_mode() {
        let mut g = TestGraph::default();
        let class = g.add_vertex(false, 0, || None);
        let x = g.add_vertex(false, 0, || None);
        let expensive_leaf = g.add_vertex(true, 5, || None);
        let cheap_input = g.add_vertex(true, 1, || None);
        g.set_roots([class]);
        g.add_edges(class, [expensive_leaf, cheap_input]);
        g.add_edges(cheap_input, [x]);

        let validity = filter(&g, FilterMode::AtLeastOneSolution);
        assert!(validity.disabled[cheap_input]);
        assert!(!validity.disabled[expensive_leaf]);

        let optimal = filter(&g, FilterMode::AtLeastOneOptimal);
        assert!(!optimal.disabled[cheap_input]);
        assert!(!optimal.disabled[expensive_leaf]);
    }

    #[test]
    fn duplicate_inputs_keep_cheapest() {
        let mut g = TestGraph::default();
        let class = g.add_vertex(false, 0, || None);
        let x = g.add_vertex(false, 0, || None);
        let expensive = g.add_vertex(true, 3, || None);
        let cheap = g.add_vertex(true, 1, || None);
        g.set_roots([class]);
        g.add_edges(class, [expensive, cheap]);
        g.add_edges(expensive, [x]);
        g.add_edges(cheap, [x]);

        for mode in [
            FilterMode::AtLeastOneSolution,
            FilterMode::AtLeastOneOptimal,
        ] {
            let report = filter(&g, mode);
            assert!(!report.disabled[cheap], "{mode:?}");
            assert!(report.disabled[expensive], "{mode:?}");
            assert_eq!(report.removed_duplicate, 1, "{mode:?}");
        }
    }

    #[test]
    fn naive_ignores_disabled_alternatives() {
        let mut g = TestGraph::default();
        let class = g.add_vertex(false, 0, || None);
        let x = g.add_vertex(false, 0, || None);
        let leaf = g.add_vertex(true, 1, || None);
        let expression = g.add_vertex(true, 5, || None);
        g.set_roots([class]);
        g.add_edges(class, [leaf, expression]);
        g.add_edges(expression, [x]);

        let report = filter(&g, FilterMode::AtLeastOneSolution);
        assert!(report.disabled[expression]);
        g.set_disabled(report.disabled.clone());

        // Without masking by the domain, the solver can satisfy `class` with the
        // disabled expression and report cost 0 instead of selecting the leaf.
        let solution = solve_naive(&mut g).unwrap();
        assert!(
            !solution.assignment[expression],
            "disabled alternative must stay unselected"
        );
        assert_eq!(solution.cost, 1);
    }

    #[test]
    fn reduced_model_lifts_to_full_graph() {
        let mut g = TestGraph::default();
        let class = g.add_vertex(false, 0, || None);
        let x = g.add_vertex(false, 0, || None);
        let leaf = g.add_vertex(true, 5, || None);
        let input = g.add_vertex(true, 1, || None);
        g.set_roots([class]);
        g.add_edges(class, [leaf, input]);
        g.add_edges(input, [x]);

        let report = filter(&g, FilterMode::AtLeastOneOptimal);
        g.set_disabled(report.disabled.clone());
        let solution = solve_naive(&mut g).unwrap();

        let mut lifted = solution.assignment.clone();
        for (id, &disabled) in report.disabled.iter().enumerate() {
            if disabled {
                lifted[id] = false;
            }
        }
        g.set_disabled(vec![]);
        assert!(g.validate(&lifted));
    }

    #[test]
    fn optimal_filter_preserves_cost() {
        let mut g = TestGraph::default();
        let class = g.add_vertex(false, 0, || None);
        let x = g.add_vertex(false, 0, || None);
        let expensive_leaf = g.add_vertex(true, 5, || None);
        let cheap_input = g.add_vertex(true, 1, || None);
        g.set_roots([class]);
        g.add_edges(class, [expensive_leaf, cheap_input]);
        g.add_edges(cheap_input, [x]);

        let report = filter(&g, FilterMode::AtLeastOneOptimal);
        g.set_disabled(report.disabled);
        let filtered = solve_naive(&mut g).unwrap().cost;

        g.set_disabled(vec![]);
        let full = solve_naive(&mut g).unwrap().cost;
        assert_eq!(filtered, full);
    }
}
