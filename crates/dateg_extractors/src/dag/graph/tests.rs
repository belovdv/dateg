use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
use std::time::Instant;

use petgraph::dot::{Config, Dot};
use rand::{
    Rng, SeedableRng,
    seq::{IndexedRandom, SliceRandom},
};
use rayon::iter::{IntoParallelIterator, ParallelIterator};

use crate::dag::{ExtractError, ExtractStats, StatValue, StatsSink};

use super::*;

fn maxsat() -> SolverConfig {
    SolverConfig::MaxSatIncScc(Default::default())
}

fn sat() -> SolverConfig {
    SolverConfig::SatIncScc(Default::default())
}

fn maxsat_scc() -> SolverConfig {
    SolverConfig::MaxSatIncScc(MaxSatIncSccConfig {
        cycle_blocking: CycleBlocking::Scc,
        ..Default::default()
    })
}

fn sat_scc() -> SolverConfig {
    SolverConfig::SatIncScc(SatIncSccConfig {
        cycle_blocking: CycleBlocking::Scc,
        ..Default::default()
    })
}

fn solve(g: &mut Graph) -> Solution {
    g.solve_with(&maxsat(), None, &mut StatsSink(None)).unwrap()
}

fn solve_sat_with(g: &mut Graph, config: &SolverConfig) -> Solution {
    g.solve_with(config, None, &mut StatsSink(None)).unwrap()
}

fn solve_sat(g: &mut Graph) -> Solution {
    solve_sat_with(g, &sat())
}

/// Cost of the selected subgraph reachable from the roots. The `Sat` solver
/// must match this, not just `assignment_cost`: a model may otherwise select
/// extra disconnected nodes and over-report its cost.
fn selected_reachable_cost(g: &Graph, assignment: &[bool]) -> usize {
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

fn solve_naive(g: &mut Graph) -> Result<Solution, ExtractError> {
    g.solve_with(&SolverConfig::Naive, None, &mut StatsSink(None))
}

impl ExtractError {
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
    let mut g = Graph::default();
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

    let mut g = Graph::default();
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

    let mut g = Graph::default();
    let root = g.add_vertex(true, 0, || None);
    let x = g.add_vertex(false, 100, || None);
    let y = g.add_vertex(true, 0, || None);
    g.set_roots([root]);
    g.add_edges(x, [y]);
    let assignment = solve(&mut g).assignment;
    assert_eq!(assignment, vec![true, false, false]);

    let mut g = Graph::default();
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
        let mut g = Graph::default();

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

        g.collect_usage();
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
    });
    let failed = count_fail_to_build.load(SeqCst);
    assert!(num - failed > 100, "only {} succeeded", num - failed);
}

#[test]
fn solver_config_and_stats() {
    let mut g = Graph::default();
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
    assert!(matches!(failure, ExtractError::Timeout));

    let mut g = Graph::default();
    let root = g.add_vertex(true, 0, || None);
    let leaf = g.add_vertex(false, 1, || None);
    g.set_roots([root]);
    g.add_edges(root, [leaf]);
    let failure = solve_naive(&mut g).unwrap_err();
    assert!(matches!(failure, ExtractError::Unsolved));
}

#[test]
fn sat_zero_cost() {
    let mut g = Graph::default();
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
    assert_eq!(stats.custom["sat.inc_scc.probes"], StatValue::Count(0));
    assert_eq!(stats.custom["sat.inc_scc.optimal"], StatValue::Count(1));
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
    ) -> Graph {
        let mut new_id = vec![0; attrs.len()];
        for (index, &old) in perm.iter().enumerate() {
            new_id[old] = index;
        }
        let mut g = Graph::default();
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

    fn run_maxsat(g: &mut Graph) -> (usize, Vec<bool>) {
        let solution = g.solve_with(&maxsat(), None, &mut StatsSink(None)).unwrap();
        (solution.cost, solution.assignment)
    }

    fn run_naive(g: &mut Graph) -> (usize, Vec<bool>) {
        let solution = solve_naive(g).unwrap();
        (solution.cost, solution.assignment)
    }

    fn run_sat(g: &mut Graph) -> (usize, Vec<bool>) {
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
    use crate::dag::graph::filtration::{FilterMode, filter};

    #[test]
    fn removes_self_referential_alternative() {
        let mut g = Graph::default();
        let class = g.add_vertex(false, 0, || None);
        let store = g.add_vertex(true, 1, || None);
        let leaf = g.add_vertex(true, 1, || None);
        g.set_roots([class]);
        g.add_edges(class, [store, leaf]);
        g.add_edges(store, [class]);

        let report = filter(&g, FilterMode::Validity);
        assert!(report.disabled[store]);
        assert!(!report.disabled[leaf]);
        assert_eq!(report.removed_self, 1);
    }

    #[test]
    fn keeps_consumers() {
        let mut g = Graph::default();
        let class = g.add_vertex(false, 0, || None);
        let mem = g.add_vertex(false, 0, || None);
        let store = g.add_vertex(true, 1, || None);
        let pure = g.add_vertex(true, 1, || None);
        g.set_roots([class]);
        g.add_edges(class, [store, pure]);
        g.add_edges(store, [mem]);
        g.set_consumes(store, [mem]);

        let report = filter(&g, FilterMode::Validity);
        assert!(!report.disabled[store]);
        assert_eq!(report.consumers, 1);
    }

    #[test]
    fn subset_domination_respects_mode() {
        let mut g = Graph::default();
        let class = g.add_vertex(false, 0, || None);
        let x = g.add_vertex(false, 0, || None);
        let expensive_leaf = g.add_vertex(true, 5, || None);
        let cheap_input = g.add_vertex(true, 1, || None);
        g.set_roots([class]);
        g.add_edges(class, [expensive_leaf, cheap_input]);
        g.add_edges(cheap_input, [x]);

        let validity = filter(&g, FilterMode::Validity);
        assert!(validity.disabled[cheap_input]);
        assert!(!validity.disabled[expensive_leaf]);

        let optimal = filter(&g, FilterMode::Optimal);
        assert!(!optimal.disabled[cheap_input]);
        assert!(!optimal.disabled[expensive_leaf]);
    }

    #[test]
    fn duplicate_inputs_keep_cheapest() {
        let mut g = Graph::default();
        let class = g.add_vertex(false, 0, || None);
        let x = g.add_vertex(false, 0, || None);
        let expensive = g.add_vertex(true, 3, || None);
        let cheap = g.add_vertex(true, 1, || None);
        g.set_roots([class]);
        g.add_edges(class, [expensive, cheap]);
        g.add_edges(expensive, [x]);
        g.add_edges(cheap, [x]);

        for mode in [FilterMode::Validity, FilterMode::Optimal] {
            let report = filter(&g, mode);
            assert!(!report.disabled[cheap], "{mode:?}");
            assert!(report.disabled[expensive], "{mode:?}");
            assert_eq!(report.removed_duplicate, 1, "{mode:?}");
        }
    }

    #[test]
    fn naive_ignores_disabled_alternatives() {
        let mut g = Graph::default();
        let class = g.add_vertex(false, 0, || None);
        let x = g.add_vertex(false, 0, || None);
        let leaf = g.add_vertex(true, 1, || None);
        let expression = g.add_vertex(true, 5, || None);
        g.set_roots([class]);
        g.add_edges(class, [leaf, expression]);
        g.add_edges(expression, [x]);

        let report = filter(&g, FilterMode::Validity);
        assert!(report.disabled[expression]);
        g.set_disabled(report.disabled.clone());

        // Without masking by `used`, the solver can satisfy `class` with the
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
        let mut g = Graph::default();
        let class = g.add_vertex(false, 0, || None);
        let x = g.add_vertex(false, 0, || None);
        let leaf = g.add_vertex(true, 5, || None);
        let input = g.add_vertex(true, 1, || None);
        g.set_roots([class]);
        g.add_edges(class, [leaf, input]);
        g.add_edges(input, [x]);

        let report = filter(&g, FilterMode::Optimal);
        g.set_disabled(report.disabled.clone());
        let solution = solve_naive(&mut g).unwrap();

        let mut lifted = solution.assignment.clone();
        for (id, &disabled) in report.disabled.iter().enumerate() {
            if disabled {
                lifted[id] = false;
            }
        }
        g.set_disabled(vec![]);
        g.collect_usage();
        assert!(g.validate(&lifted));
    }

    #[test]
    fn optimal_filter_preserves_cost() {
        let mut g = Graph::default();
        let class = g.add_vertex(false, 0, || None);
        let x = g.add_vertex(false, 0, || None);
        let expensive_leaf = g.add_vertex(true, 5, || None);
        let cheap_input = g.add_vertex(true, 1, || None);
        g.set_roots([class]);
        g.add_edges(class, [expensive_leaf, cheap_input]);
        g.add_edges(cheap_input, [x]);

        let report = filter(&g, FilterMode::Optimal);
        g.set_disabled(report.disabled);
        let filtered = solve_naive(&mut g).unwrap().cost;

        g.set_disabled(vec![]);
        let full = solve_naive(&mut g).unwrap().cost;
        assert_eq!(filtered, full);
    }
}
