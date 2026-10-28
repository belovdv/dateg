//! Bottom-up recovery for pure components.
//!
//! Each pure component is reduced to one derivation per boundary output. The
//! component is ranked bottom-up from its opaque inputs and constants; a value
//! then keeps the producer with the fewest inputs, then lowest cost, among the
//! producers whose inputs have a strictly lower rank. The rank restriction
//! makes the kept producers acyclic. Every other pure constructor in the
//! component is disabled, so each kept value has exactly one enabled producer.
//!
//! This is a validity-only reduction: it loses optimality, and it can lose
//! solvability when the chosen route through opaque inputs is not realizable
//! because of read-write/`consumes` interactions elsewhere. Callers must not
//! treat a downstream `Infeasible` as a proof while this reduction is active.
//! It is meant to run after [`super::dead_vertices`], which removes boundary
//! inputs that have no derivation at all.

use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

use petgraph::{
    Direction::{Incoming, Outgoing},
    graph::NodeIndex,
};

use super::{Graph, VertexId, components};

#[derive(Clone, Debug, Default)]
pub(crate) struct Report {
    pub(crate) disabled: Vec<bool>,
    pub(crate) components: usize,
    pub(crate) vertices: usize,
    pub(crate) needed: usize,
    pub(crate) recovered_values: usize,
    pub(crate) recovered_constructors: usize,
    pub(crate) collapsed_components: usize,
    /// Needed values without a recovered route; the component is left alone.
    pub(crate) missing: usize,
    pub(crate) disabled_total: usize,
    pub(crate) duration: Duration,
}

fn is_value(graph: &Graph, vertex: VertexId) -> bool {
    !graph.graph[NodeIndex::new(vertex)].is_all
}

fn is_pure_constructor(graph: &Graph, vertex: VertexId) -> bool {
    let node = &graph.graph[NodeIndex::new(vertex)];
    node.is_all && node.consumes.is_empty()
}

fn inputs(graph: &Graph, constructor: VertexId) -> impl Iterator<Item = VertexId> + '_ {
    graph
        .graph
        .neighbors_directed(NodeIndex::new(constructor), Outgoing)
        .map(|node| node.index())
}

fn producers(graph: &Graph, value: VertexId) -> impl Iterator<Item = VertexId> + '_ {
    graph
        .graph
        .neighbors_directed(NodeIndex::new(value), Outgoing)
        .map(|node| node.index())
}

fn consumers(graph: &Graph, value: VertexId) -> impl Iterator<Item = VertexId> + '_ {
    graph
        .graph
        .neighbors_directed(NodeIndex::new(value), Incoming)
        .map(|node| node.index())
}

fn outputs(graph: &Graph, constructor: VertexId) -> impl Iterator<Item = VertexId> + '_ {
    graph
        .graph
        .neighbors_directed(NodeIndex::new(constructor), Incoming)
        .map(|node| node.index())
        .filter(|&node| is_value(graph, node))
}

fn cost(graph: &Graph, vertex: VertexId) -> usize {
    graph.graph[NodeIndex::new(vertex)].cost
}

/// No explicit deadline: the pass is O(V+E) and is used as a last resort, after
/// the normal budget may already be exhausted.
///
/// `alternatives` is the number of producers kept per value. `1` recovers a
/// single route; larger values keep the best `K` rank-valid producers per value.
/// The rank condition still makes the kept subgraph acyclic. This mode is lossy
/// through opaque inputs; the support-frontier mode is the validity-preserving
/// alternative.
pub(crate) fn filter(
    graph: &Graph,
    roots: &[VertexId],
    enabled: &[bool],
    alternatives: usize,
) -> Report {
    let alternatives = alternatives.max(1);
    let started = Instant::now();
    let count = graph.graph.node_count();
    let parts = components::decompose(graph);
    let mut report = Report {
        disabled: vec![false; count],
        components: parts.len(),
        ..Report::default()
    };
    let mut local = vec![usize::MAX; count];
    let mut disabled = vec![false; count];

    for part in &parts {
        let len = part.vertices.len();
        report.vertices += len;
        for (index, &vertex) in part.vertices.iter().enumerate() {
            local[vertex] = index;
        }

        let mut is_input = vec![false; len];
        for &input in &part.inputs {
            if local[input] != usize::MAX {
                is_input[local[input]] = true;
            }
        }
        let mut protected = vec![false; len];
        for &vertex in part
            .inputs
            .iter()
            .chain(part.outputs.iter())
            .chain(roots.iter())
        {
            if local[vertex] != usize::MAX {
                protected[local[vertex]] = true;
            }
        }

        let mut rank = vec![u32::MAX; len];
        let mut usable = vec![false; len];
        let mut missing = vec![u32::MAX; len];
        let mut chosen: Vec<Vec<VertexId>> = vec![Vec::new(); len];
        let mut queue = VecDeque::new();

        for index in 0..len {
            let vertex = part.vertices[index];
            if !enabled[vertex] {
                continue;
            }
            if is_value(graph, vertex) {
                if is_input[index] {
                    rank[index] = 0;
                }
                continue;
            }
            if !is_pure_constructor(graph, vertex) {
                continue;
            }
            let mut pending = 0u32;
            let mut valid = true;
            for input in inputs(graph, vertex) {
                let Some(input_index) = local.get(input).copied() else {
                    valid = false;
                    break;
                };
                if input_index == usize::MAX || !enabled[input] {
                    valid = false;
                    break;
                }
                if rank[input_index] == u32::MAX {
                    pending += 1;
                }
            }
            if !valid {
                continue;
            }
            usable[index] = true;
            missing[index] = pending;
            if pending == 0 {
                queue.push_back(index);
            }
        }

        while let Some(index) = queue.pop_front() {
            let vertex = part.vertices[index];
            let mut input_rank = 0u32;
            for input in inputs(graph, vertex) {
                input_rank = input_rank.max(rank[local[input]]);
            }
            let constructor_rank = input_rank.saturating_add(1);
            for output in outputs(graph, vertex) {
                let Some(output_index) = local.get(output).copied() else {
                    continue;
                };
                if output_index == usize::MAX || rank[output_index] != u32::MAX {
                    continue;
                }
                rank[output_index] = constructor_rank;
                for consumer in consumers(graph, output) {
                    let Some(consumer_index) = local.get(consumer).copied() else {
                        continue;
                    };
                    if consumer_index == usize::MAX
                        || !usable[consumer_index]
                        || missing[consumer_index] == 0
                    {
                        continue;
                    }
                    missing[consumer_index] -= 1;
                    if missing[consumer_index] == 0 {
                        queue.push_back(consumer_index);
                    }
                }
            }
        }

        for index in 0..len {
            let vertex = part.vertices[index];
            if !enabled[vertex] || !is_value(graph, vertex) || is_input[index] {
                continue;
            }
            if rank[index] == u32::MAX {
                continue;
            }
            let mut best: Vec<(usize, usize, VertexId)> = Vec::new();
            for producer in producers(graph, vertex) {
                let Some(producer_index) = local.get(producer).copied() else {
                    continue;
                };
                if producer_index == usize::MAX
                    || !usable[producer_index]
                    || !is_pure_constructor(graph, producer)
                {
                    continue;
                }
                let mut valid = true;
                for input in inputs(graph, producer) {
                    let Some(input_index) = local.get(input).copied() else {
                        valid = false;
                        break;
                    };
                    if input_index == usize::MAX
                        || rank[input_index] == u32::MAX
                        || rank[input_index] >= rank[index]
                    {
                        valid = false;
                        break;
                    }
                }
                if !valid {
                    continue;
                }
                best.push((
                    inputs(graph, producer).count(),
                    cost(graph, producer),
                    producer,
                ));
            }
            best.sort_unstable();
            best.truncate(alternatives);
            chosen[index] = best.into_iter().map(|(_, _, producer)| producer).collect();
        }

        let mut needed: Vec<usize> = part
            .outputs
            .iter()
            .chain(roots.iter())
            .filter_map(|&vertex| {
                let index = local[vertex];
                (index != usize::MAX && enabled[vertex]).then_some(index)
            })
            .collect();
        report.needed += needed.len();

        let mut selected_value = vec![false; len];
        let mut selected_constructor = vec![false; len];
        let mut component_missing = 0usize;
        let mut stack = std::mem::take(&mut needed);
        while let Some(index) = stack.pop() {
            let vertex = part.vertices[index];
            if !enabled[vertex] || selected_value[index] {
                continue;
            }
            selected_value[index] = true;
            if is_input[index] {
                continue;
            }
            let producers = std::mem::take(&mut chosen[index]);
            if producers.is_empty() {
                component_missing += 1;
                continue;
            }
            for producer in producers {
                let producer_index = local[producer];
                if producer_index == usize::MAX {
                    component_missing += 1;
                    continue;
                }
                if selected_constructor[producer_index] {
                    continue;
                }
                selected_constructor[producer_index] = true;
                for input in inputs(graph, producer) {
                    let Some(input_index) = local.get(input).copied() else {
                        component_missing += 1;
                        continue;
                    };
                    if input_index == usize::MAX {
                        component_missing += 1;
                    } else {
                        stack.push(input_index);
                    }
                }
            }
        }

        if component_missing > 0 {
            report.missing += component_missing;
        } else {
            let mut collapsed = false;
            for index in 0..len {
                let vertex = part.vertices[index];
                if !enabled[vertex] || disabled[vertex] {
                    continue;
                }
                let remove = if is_value(graph, vertex) {
                    !selected_value[index] && !protected[index]
                } else {
                    !selected_constructor[index]
                };
                if remove {
                    disabled[vertex] = true;
                    report.disabled_total += 1;
                    collapsed = true;
                }
            }
            report.recovered_values += selected_value.iter().filter(|&&selected| selected).count();
            report.recovered_constructors += selected_constructor
                .iter()
                .filter(|&&selected| selected)
                .count();
            if collapsed {
                report.collapsed_components += 1;
            }
        }

        for &vertex in &part.vertices {
            local[vertex] = usize::MAX;
        }
    }

    report.disabled = disabled;
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
        constructor_cost(graph, 1, inputs)
    }

    fn constructor_cost(graph: &mut Graph, cost: usize, inputs: &[VertexId]) -> VertexId {
        let constructor = graph.add_vertex(true, cost, || None);
        graph.add_edges(constructor, inputs.iter().copied());
        constructor
    }

    fn produce(graph: &mut Graph, value: VertexId, constructor: VertexId) {
        graph.add_edges(value, [constructor]);
    }

    fn constant(graph: &mut Graph) -> VertexId {
        let value = value(graph);
        let constant = constructor_cost(graph, 0, &[]);
        produce(graph, value, constant);
        value
    }

    fn run(graph: &Graph, roots: &[VertexId]) -> Report {
        filter(graph, roots, &vec![true; graph.graph.node_count()], 1)
    }

    fn run_wide(graph: &Graph, roots: &[VertexId], alternatives: usize) -> Report {
        filter(
            graph,
            roots,
            &vec![true; graph.graph.node_count()],
            alternatives,
        )
    }

    fn enabled_producers(graph: &Graph, report: &Report, value: VertexId) -> Vec<VertexId> {
        producers(graph, value)
            .filter(|&producer| !report.disabled[producer])
            .collect()
    }

    #[test]
    fn associative_routes_keep_one_producer_per_value() {
        let mut graph = Graph::default();
        let a = constant(&mut graph);
        let b = constant(&mut graph);
        let c = constant(&mut graph);

        let bc = value(&mut graph);
        let make_bc = constructor(&mut graph, &[b, c]);
        produce(&mut graph, bc, make_bc);

        let ab = value(&mut graph);
        let make_ab = constructor(&mut graph, &[a, b]);
        produce(&mut graph, ab, make_ab);

        let root = value(&mut graph);
        let left = constructor(&mut graph, &[a, bc]);
        let right = constructor(&mut graph, &[ab, c]);
        produce(&mut graph, root, left);
        produce(&mut graph, root, right);

        let report = run(&graph, &[root]);
        assert_eq!(report.missing, 0);
        assert_eq!(enabled_producers(&graph, &report, root).len(), 1);
        let kept = [
            enabled_producers(&graph, &report, bc).len(),
            enabled_producers(&graph, &report, ab).len(),
        ];
        assert!(kept.iter().all(|&count| count <= 1), "kept {kept:?}");
        assert_eq!(kept.iter().sum::<usize>(), 1, "exactly one route stays");
        assert!(report.disabled_total > 0);
    }

    #[test]
    fn a_cyclic_alternative_is_not_chosen() {
        let mut graph = Graph::default();
        let a = constant(&mut graph);
        let x = value(&mut graph);
        let y = value(&mut graph);
        let root = value(&mut graph);

        let base = constructor(&mut graph, &[a]);
        produce(&mut graph, x, base);
        let make_y = constructor(&mut graph, &[x, a]);
        produce(&mut graph, y, make_y);
        let cyclic = constructor(&mut graph, &[y, a]);
        produce(&mut graph, x, cyclic);
        let root_use = constructor(&mut graph, &[x, y]);
        produce(&mut graph, root, root_use);

        let report = run(&graph, &[root]);
        assert_eq!(report.missing, 0);
        assert!(!report.disabled[base]);
        assert!(report.disabled[cyclic]);
        assert_eq!(enabled_producers(&graph, &report, x), vec![base]);
    }

    #[test]
    fn fewest_inputs_wins_over_cost() {
        let mut graph = Graph::default();
        let a = constant(&mut graph);
        let b = constant(&mut graph);
        let x = value(&mut graph);
        let root = value(&mut graph);

        let multi = constructor_cost(&mut graph, 1, &[a, b]);
        let unary = constructor_cost(&mut graph, 5, &[a]);
        produce(&mut graph, x, multi);
        produce(&mut graph, x, unary);
        let root_use = constructor(&mut graph, &[x]);
        produce(&mut graph, root, root_use);

        let report = run(&graph, &[root]);
        assert_eq!(enabled_producers(&graph, &report, x), vec![unary]);
        assert!(report.disabled[multi]);
    }

    #[test]
    fn cost_breaks_equal_input_counts() {
        let mut graph = Graph::default();
        let a = constant(&mut graph);
        let x = value(&mut graph);
        let root = value(&mut graph);

        let cheap = constructor_cost(&mut graph, 1, &[a]);
        let expensive = constructor_cost(&mut graph, 5, &[a]);
        produce(&mut graph, x, cheap);
        produce(&mut graph, x, expensive);
        let root_use = constructor(&mut graph, &[x]);
        produce(&mut graph, root, root_use);

        let report = run(&graph, &[root]);
        assert_eq!(enabled_producers(&graph, &report, x), vec![cheap]);
    }

    #[test]
    fn opaque_inputs_are_kept_as_leaves() {
        let mut graph = Graph::default();
        let memory = value(&mut graph);
        let input = value(&mut graph);
        let load = graph.add_vertex(true, 1, || None);
        graph.set_consumes(load, [memory]);
        produce(&mut graph, input, load);

        let root = value(&mut graph);
        let add = constructor(&mut graph, &[input, input]);
        produce(&mut graph, root, add);
        let store = graph.add_vertex(true, 1, || None);
        graph.set_consumes(store, [memory]);
        graph.add_edges(store, [root]);

        let report = run(&graph, &[root]);
        assert_eq!(report.missing, 0);
        assert!(!report.disabled[input]);
        assert!(!report.disabled[add]);
        assert!(!report.disabled[load]);
        assert!(!report.disabled[store]);
    }

    #[test]
    fn missing_routes_leave_the_component_untouched() {
        let mut graph = Graph::default();
        let a = constant(&mut graph);
        let x = value(&mut graph);
        let y = value(&mut graph);
        let root = value(&mut graph);

        // x and y only have a cyclic derivation; the component is unfounded.
        let make_x = constructor(&mut graph, &[y, a]);
        produce(&mut graph, x, make_x);
        let make_y = constructor(&mut graph, &[x, a]);
        produce(&mut graph, y, make_y);
        let root_use = constructor(&mut graph, &[x, y]);
        produce(&mut graph, root, root_use);

        let report = run(&graph, &[root]);
        assert!(report.missing > 0);
        assert_eq!(report.disabled_total, 0);
        assert!(!report.disabled[make_x] && !report.disabled[make_y]);
    }

    #[test]
    fn wide_mode_keeps_multiple_routes_and_stays_acyclic() {
        let mut graph = Graph::default();
        let a = constant(&mut graph);
        let b = constant(&mut graph);
        let c = constant(&mut graph);

        let bc = value(&mut graph);
        let make_bc = constructor(&mut graph, &[b, c]);
        produce(&mut graph, bc, make_bc);

        let ab = value(&mut graph);
        let make_ab = constructor(&mut graph, &[a, b]);
        produce(&mut graph, ab, make_ab);

        let root = value(&mut graph);
        let left = constructor(&mut graph, &[a, bc]);
        let right = constructor(&mut graph, &[ab, c]);
        produce(&mut graph, root, left);
        produce(&mut graph, root, right);

        let report = run_wide(&graph, &[root], usize::MAX);
        assert_eq!(report.missing, 0);
        assert_eq!(enabled_producers(&graph, &report, root).len(), 2);
        assert!(!report.disabled[left] && !report.disabled[right]);
        let enabled: Vec<bool> = report.disabled.iter().map(|&disabled| !disabled).collect();
        assert!(graph.is_acyclic(&enabled));
    }

    #[test]
    fn random_closed_components_stay_acyclic() {
        use rand::{Rng, SeedableRng, seq::IndexedRandom};

        for seed in 0..64u64 {
            let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
            let mut graph = Graph::default();
            let count = rng.random_range(4..=10);
            let mut values = Vec::new();
            for _ in 0..count {
                values.push(value(&mut graph));
            }
            for (index, &value) in values.iter().enumerate() {
                let base = constructor_cost(&mut graph, rng.random_range(0..=5), &[]);
                produce(&mut graph, value, base);
                let mut connect = index > 0;
                for _ in 0..rng.random_range(1..=3) {
                    let arity = rng.random_range(1..=3);
                    let mut inputs: Vec<VertexId> = (0..arity)
                        .map(|_| *values.choose(&mut rng).expect("a value"))
                        .collect();
                    if connect {
                        inputs[0] = *values[..index].choose(&mut rng).expect("earlier value");
                        connect = false;
                    }
                    let constructor =
                        constructor_cost(&mut graph, rng.random_range(0..=5), &inputs);
                    produce(&mut graph, value, constructor);
                }
            }
            let root = *values.last().expect("a value");
            for width in [1usize, 4, usize::MAX] {
                let report = run_wide(&graph, &[root], width);
                assert_eq!(report.missing, 0, "seed {seed}, width {width}");
                let enabled: Vec<bool> =
                    report.disabled.iter().map(|&disabled| !disabled).collect();
                assert!(
                    graph.is_acyclic(&enabled),
                    "seed {seed}, width {width} left a cycle"
                );
                for &value in &values {
                    if !enabled[value] {
                        continue;
                    }
                    let kept = enabled_producers(&graph, &report, value);
                    assert!(
                        kept.len() <= width,
                        "seed {seed}, width {width}: value {value} has {kept:?}"
                    );
                }
                assert!(
                    !enabled_producers(&graph, &report, root).is_empty(),
                    "seed {seed}, width {width}: root lost its producer"
                );
            }
        }
    }
}
