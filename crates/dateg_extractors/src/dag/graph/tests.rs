use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};

use petgraph::dot::{Config, Dot};
use rand::{
    Rng, SeedableRng,
    seq::{IndexedRandom, SliceRandom},
};
use rayon::iter::{IntoParallelIterator, ParallelIterator};

use super::*;

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
    let assignment = g.solve(SolverConfig::MaxSat {});
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
    let assignment = g.solve(SolverConfig::MaxSat {});
    assert_eq!(assignment, vec![true, true, false, true, true, true]);

    let mut g = Graph::default();
    let root = g.add_vertex(true, 0, || None);
    let x = g.add_vertex(false, 100, || None);
    let y = g.add_vertex(true, 0, || None);
    g.set_roots([root]);
    g.add_edges(x, [y]);
    let assignment = g.solve(SolverConfig::MaxSat {});
    assert_eq!(assignment, vec![true, false, false]);

    let mut g = Graph::default();
    let root = g.add_vertex(false, 0, || None);
    let a = g.add_vertex(true, 1, || None);
    let b = g.add_vertex(true, 0, || None);
    let c = g.add_vertex(true, 0, || None);
    g.set_roots([root]);
    g.add_edges(root, [a, b, c]);
    g.add_conflicting_group([a, b, c]);
    let solution = g.solve(SolverConfig::MaxSat {});
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
        let (cost, _) = solve_naive::solve(&g);
        if cost == usize::MAX {
            dbg!(&g.graph);
            let dot = Dot::with_config(&g.graph, &[Config::EdgeNoLabel, Config::NodeIndexLabel]);
            eprintln!("{dot:?}",);
            eprintln!("{:?}", assignment);
            panic!()
        }
        let maxsat_assignment = g.solve(SolverConfig::MaxSat {});

        let maxsat_cost = g.cost(&maxsat_assignment);

        assert!(g.check_constraints(&maxsat_assignment), "test {i}: invalid");

        assert_eq!(cost, maxsat_cost, "test {}: {} != {}", i, cost, maxsat_cost);
    });
    let failed = count_fail_to_build.load(SeqCst);
    assert!(num - failed > 100, "only {} succeeded", num - failed);
}
