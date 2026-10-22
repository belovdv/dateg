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
    SolverConfig::MaxSatIncSccBlocking(Default::default())
}

fn solve(g: &mut Graph) -> Solution {
    g.solve_with(&maxsat(), None, &mut StatsSink(None)).unwrap()
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

/// The same abstract graph has multiple optimal assignments when built in
/// different orders: both MaxSAT and the non-z3 exhaustive solver pick different
/// ones. Once the construction order is fixed, repeated extractions must agree.
/// This makes the determinism assertion non-vacuous.
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

    assert_eq!(maxsat_identity.0, maxsat_reordered.0);
    assert_ne!(maxsat_identity.1, maxsat_reordered.1);
    assert_eq!(naive_identity.0, naive_reordered.0);
    assert_ne!(naive_identity.1, naive_reordered.1);

    for _ in 0..4 {
        let mut g = build(&identity, &attrs, &edges, &groups);
        assert_eq!(maxsat_identity, run_maxsat(&mut g));
        let mut g = build(&identity, &attrs, &edges, &groups);
        assert_eq!(naive_identity, run_naive(&mut g));
    }
}
