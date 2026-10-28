//! Trivial structural cleanup.
//!
//! Keeps only vertices that lie in the root-reachable domain and that have a
//! finite acyclic derivation:
//!
//! - a value is producible when one of its enabled producers is producible;
//! - a constructor is producible when it is enabled and all of its inputs are
//!   producible;
//! - enabled constructors without inputs seed the fixpoint.
//!
//! Unreachable vertices can only be extra selections (never needed by the
//! root), and unproducible vertices cannot be selected by any valid assignment.
//! Removing either preserves at least one optimal solution. Conflict groups
//! and `consumes` are ignored, so producibility is an over-approximation: a
//! value that is not producible definitely cannot be used, but a producible
//! value may still be infeasible. A dead root is therefore a proof of
//! infeasibility.
//!
//! The pass is O(V+E) and does not allocate per-vertex bitsets.

use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

use petgraph::graph::NodeIndex;

use super::{Graph, VertexId, View, reachable};

#[derive(Clone, Debug, Default)]
pub(crate) struct Report {
    pub(crate) disabled: Vec<bool>,
    pub(crate) removed_unreachable: usize,
    pub(crate) removed_unproducible: usize,
    pub(crate) root_dead: bool,
    pub(crate) timed_out: bool,
    pub(crate) duration: Duration,
}

pub(crate) fn filter(
    graph: &Graph,
    roots: &[VertexId],
    enabled: &[bool],
    deadline: Option<Instant>,
) -> Report {
    let started = Instant::now();
    let count = graph.graph.node_count();
    let mut report = Report {
        disabled: vec![false; count],
        ..Report::default()
    };
    if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
        report.timed_out = true;
        report.duration = started.elapsed();
        return report;
    }

    let view = View::new(graph, enabled.to_vec(), Vec::new());
    let domain = reachable(graph, roots, &view);
    let mut missing = vec![u32::MAX; count];
    let mut constructor_alive = vec![false; count];
    let mut value_alive = vec![false; count];
    let mut queue = VecDeque::new();

    for node in graph.graph.node_indices() {
        let constructor = node.index();
        if !graph.graph[node].is_all || !view.enabled[constructor] || !domain[constructor] {
            continue;
        }
        let mut inputs = 0u32;
        let mut usable = true;
        for input in graph.graph.neighbors(node) {
            let input_id = input.index();
            if !view.enabled[input_id] || !domain[input_id] {
                usable = false;
                break;
            }
            inputs += 1;
        }
        if !usable {
            continue;
        }
        missing[constructor] = inputs;
        if inputs == 0 {
            constructor_alive[constructor] = true;
            queue.push_back(constructor);
        }
    }

    while let Some(constructor) = queue.pop_front() {
        for output in graph
            .graph
            .neighbors_directed(NodeIndex::new(constructor), petgraph::Direction::Incoming)
        {
            let output = output.index();
            if graph.graph[NodeIndex::new(output)].is_all
                || !view.enabled[output]
                || !domain[output]
                || value_alive[output]
            {
                continue;
            }
            value_alive[output] = true;
            for consumer in graph
                .graph
                .neighbors_directed(NodeIndex::new(output), petgraph::Direction::Incoming)
            {
                let consumer = consumer.index();
                if !graph.graph[NodeIndex::new(consumer)].is_all || missing[consumer] == u32::MAX {
                    continue;
                }
                missing[consumer] -= 1;
                if missing[consumer] == 0 {
                    constructor_alive[consumer] = true;
                    queue.push_back(consumer);
                }
            }
        }
    }

    for node in graph.graph.node_indices() {
        let vertex = node.index();
        if !view.enabled[vertex] {
            continue;
        }
        let alive = if graph.graph[node].is_all {
            constructor_alive[vertex]
        } else {
            value_alive[vertex]
        };
        if !domain[vertex] {
            report.disabled[vertex] = true;
            report.removed_unreachable += 1;
        } else if !alive {
            report.disabled[vertex] = true;
            report.removed_unproducible += 1;
        }
    }

    report.root_dead = roots
        .iter()
        .any(|&root| !view.enabled[root] || !domain[root] || !value_alive[root]);
    report.duration = started.elapsed();
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(graph: &mut Graph) -> VertexId {
        graph.add_vertex(false, 0, || None)
    }

    fn constructor(graph: &mut Graph, inputs: &[VertexId]) -> VertexId {
        let constructor = graph.add_vertex(true, 1, || None);
        graph.add_edges(constructor, inputs.iter().copied());
        constructor
    }

    fn produce(graph: &mut Graph, value: VertexId, constructor: VertexId) {
        graph.add_edges(value, [constructor]);
    }

    fn run(graph: &Graph, roots: &[VertexId]) -> Report {
        filter(graph, roots, &vec![true; graph.graph.node_count()], None)
    }

    #[test]
    fn keeps_a_reachable_derivation_and_removes_the_rest() {
        let mut graph = Graph::default();
        let a = value(&mut graph);
        let b = value(&mut graph);
        let x = value(&mut graph);
        let root = value(&mut graph);

        let make_a = constructor(&mut graph, &[]);
        produce(&mut graph, a, make_a);
        let make_b = constructor(&mut graph, &[]);
        produce(&mut graph, b, make_b);
        let add = constructor(&mut graph, &[a, b]);
        produce(&mut graph, x, add);
        let root_add = constructor(&mut graph, &[x, a]);
        produce(&mut graph, root, root_add);

        // Not connected to the root.
        let unused = value(&mut graph);
        let unused_add = constructor(&mut graph, &[a, b]);
        produce(&mut graph, unused, unused_add);

        let report = run(&graph, &[root]);
        assert!(!report.root_dead);
        assert!(!report.disabled[root] && !report.disabled[x] && !report.disabled[a]);
        assert!(!report.disabled[add] && !report.disabled[root_add]);
        assert!(report.disabled[unused] && report.disabled[unused_add]);
        assert_eq!(report.removed_unreachable, 2);
        assert_eq!(report.removed_unproducible, 0);
    }

    #[test]
    fn a_pure_cycle_without_a_base_is_unproducible() {
        let mut graph = Graph::default();
        let a = value(&mut graph);
        let x = value(&mut graph);
        let y = value(&mut graph);
        let root = value(&mut graph);

        let make_a = constructor(&mut graph, &[]);
        produce(&mut graph, a, make_a);
        let add_y_a = constructor(&mut graph, &[y, a]);
        produce(&mut graph, x, add_y_a);
        let add_x_a = constructor(&mut graph, &[x, a]);
        produce(&mut graph, y, add_x_a);
        let root_use = constructor(&mut graph, &[x, a]);
        produce(&mut graph, root, root_use);

        let report = run(&graph, &[root]);
        assert!(report.root_dead);
        assert!(report.disabled[x] && report.disabled[y]);
        assert!(report.disabled[add_y_a] && report.disabled[add_x_a]);
        assert!(report.removed_unproducible >= 2);
    }

    #[test]
    fn a_base_alternative_makes_the_cycle_usable() {
        let mut graph = Graph::default();
        let a = value(&mut graph);
        let x = value(&mut graph);
        let y = value(&mut graph);
        let root = value(&mut graph);

        let make_a = constructor(&mut graph, &[]);
        produce(&mut graph, a, make_a);
        let add_y_a = constructor(&mut graph, &[y, a]);
        produce(&mut graph, x, add_y_a);
        let add_x_a = constructor(&mut graph, &[x, a]);
        produce(&mut graph, y, add_x_a);
        // x also has a finite way.
        let base = constructor(&mut graph, &[a]);
        produce(&mut graph, x, base);
        let root_use = constructor(&mut graph, &[y, a]);
        produce(&mut graph, root, root_use);

        let report = run(&graph, &[root]);
        assert!(!report.root_dead);
        assert!(!report.disabled[x] && !report.disabled[y] && !report.disabled[base]);
    }

    #[test]
    fn a_disabled_producer_makes_the_value_dead() {
        let mut graph = Graph::default();
        let a = value(&mut graph);
        let root = value(&mut graph);
        let make_a = constructor(&mut graph, &[]);
        produce(&mut graph, a, make_a);
        let root_use = constructor(&mut graph, &[a]);
        produce(&mut graph, root, root_use);

        let count = graph.graph.node_count();
        let mut enabled = vec![true; count];
        enabled[make_a] = false;
        let report = filter(&graph, &[root], &enabled, None);
        assert!(report.root_dead);
        assert!(report.disabled[a] && report.disabled[root_use]);
        assert!(!report.disabled[make_a]);
    }
}
