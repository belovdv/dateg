//! Support-frontier bottom-up filtration.
//!
//! For every value, keep one acyclic derivation per inclusion-minimal set of
//! component inputs (`I`) from which the value can be produced. For every
//! output support `Is -> o`, there is a kept derivation from some `S ⊆ Is`;
//! since supports are upward-closed, this preserves every derivable support.
//! That is the validity contract: the outer read-write skeleton may choose any
//! realizable input subset, and the collapsed component can still produce its
//! outputs from that subset.
//!
//! Caps bound the work. A component that exceeds a cap is left untouched: it is
//! never partially collapsed, so validity is preserved. Cycles may remain in
//! the union of routes; the solver's acyclicity encoding handles them.

use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

use petgraph::{
    Direction::{Incoming, Outgoing},
    graph::NodeIndex,
};

use super::{Graph, VertexId, components};

/// Raw unions kept per constructor before pruning.
const MAX_COMBINATIONS: usize = 1024;
/// Constructor recomputations per component before giving up.
const MAX_UPDATES: usize = 200_000;
/// Route arena entries per component.
const MAX_ROUTES: usize = 200_000;
/// Hard wall-clock cap for the whole frontier pass when the caller has no
/// deadline. The pass is a last resort; a component that does not finish is
/// left untouched.
const MAX_FRONTIER_TIME: Duration = Duration::from_millis(500);

#[derive(Clone, Debug, Default)]
pub(crate) struct Report {
    pub(crate) disabled: Vec<bool>,
    pub(crate) components: usize,
    pub(crate) vertices: usize,
    pub(crate) processed: usize,
    pub(crate) skipped_wide: usize,
    pub(crate) skipped_missing: usize,
    pub(crate) timed_out: bool,
    pub(crate) supports: usize,
    pub(crate) max_frontier: usize,
    pub(crate) routes: usize,
    pub(crate) disabled_total: usize,
    pub(crate) duration: Duration,
}

#[derive(Clone)]
struct Entry {
    support: Vec<u64>,
    route: usize,
}

struct Route {
    value: VertexId,
    producer: Option<VertexId>,
    children: Vec<usize>,
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

fn empty(width: usize) -> Vec<u64> {
    vec![0; width]
}

fn singleton(width: usize, bit: usize) -> Vec<u64> {
    let mut mask = vec![0; width];
    mask[bit / 64] |= 1u64 << (bit % 64);
    mask
}

fn subset(left: &[u64], right: &[u64]) -> bool {
    left.iter()
        .zip(right)
        .all(|(left, right)| left & !right == 0)
}

fn count(mask: &[u64]) -> usize {
    mask.iter().map(|word| word.count_ones() as usize).sum()
}

fn prune_pairs(pairs: &mut Vec<(Vec<u64>, Vec<usize>)>) {
    pairs.sort_unstable_by(|left, right| {
        (count(&left.0), &left.0, left.1.len()).cmp(&(count(&right.0), &right.0, right.1.len()))
    });
    let mut kept: Vec<(Vec<u64>, Vec<usize>)> = Vec::new();
    for pair in pairs.drain(..) {
        if kept.iter().any(|kept| subset(&kept.0, &pair.0)) {
            continue;
        }
        kept.push(pair);
    }
    *pairs = kept;
}

fn add_support(
    frontier: &mut Vec<Entry>,
    support: Vec<u64>,
    value: VertexId,
    producer: Option<VertexId>,
    children: Vec<usize>,
    routes: &mut Vec<Route>,
    max_supports: usize,
) -> Result<bool, ()> {
    if frontier
        .iter()
        .any(|entry| subset(&entry.support, &support))
    {
        return Ok(false);
    }
    if routes.len() >= MAX_ROUTES {
        return Err(());
    }
    let route = routes.len();
    routes.push(Route {
        value,
        producer,
        children,
    });
    frontier.retain(|entry| !subset(&support, &entry.support));
    frontier.push(Entry { support, route });
    if frontier.len() > max_supports {
        Err(())
    } else {
        Ok(true)
    }
}

#[derive(Clone, Copy)]
enum Abort {
    Wide,
    Deadline,
}

fn deadline_hit(deadline: Option<Instant>) -> bool {
    deadline.is_some_and(|deadline| Instant::now() >= deadline)
}

/// `max_supports` caps the minimal supports kept per value. On any cap
/// overflow the component is skipped by the caller. The pass also respects
/// `deadline`; a timed-out component is left untouched.
pub(crate) fn filter(
    graph: &Graph,
    roots: &[VertexId],
    enabled: &[bool],
    max_supports: usize,
    deadline: Option<Instant>,
) -> Report {
    let started = Instant::now();
    let own_deadline = started + MAX_FRONTIER_TIME;
    let deadline = Some(deadline.map_or(own_deadline, |deadline| deadline.min(own_deadline)));
    let count_vertices = graph.graph.node_count();
    let parts = components::decompose(graph);
    let mut report = Report {
        disabled: vec![false; count_vertices],
        components: parts.len(),
        ..Report::default()
    };
    let max_supports = max_supports.max(1);
    let mut local = vec![usize::MAX; count_vertices];
    let mut disabled = vec![false; count_vertices];

    for part in &parts {
        if deadline_hit(deadline) {
            report.timed_out = true;
            break;
        }
        let len = part.vertices.len();
        report.vertices += len;
        for (index, &vertex) in part.vertices.iter().enumerate() {
            local[vertex] = index;
        }
        let width = part.inputs.len().div_ceil(64);
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

        let built = build_component(graph, part, &local, enabled, width, max_supports, deadline);
        let (frontier, routes, max_frontier) = match built {
            Ok(built) => built,
            Err(Abort::Wide) => {
                report.skipped_wide += 1;
                for &vertex in &part.vertices {
                    local[vertex] = usize::MAX;
                }
                continue;
            }
            Err(Abort::Deadline) => {
                report.timed_out = true;
                for &vertex in &part.vertices {
                    local[vertex] = usize::MAX;
                }
                break;
            }
        };
        report.max_frontier = report.max_frontier.max(max_frontier);
        report.routes += routes.len();

        let mut needed: Vec<usize> = Vec::new();
        let mut missing = false;
        for &vertex in part.outputs.iter().chain(roots.iter()) {
            let index = local[vertex];
            if index == usize::MAX || !enabled[vertex] {
                continue;
            }
            if frontier[index].is_empty() {
                missing = true;
                break;
            }
            needed.push(index);
        }
        if missing {
            report.skipped_missing += 1;
            for &vertex in &part.vertices {
                local[vertex] = usize::MAX;
            }
            continue;
        }

        let mut selected_value = vec![false; len];
        let mut selected_constructor = vec![false; len];
        let mut seen_route = vec![false; routes.len()];
        let mut traced = 0usize;
        let mut stack: Vec<usize> = Vec::new();
        for &index in &needed {
            report.supports += frontier[index].len();
            stack.extend(frontier[index].iter().map(|entry| entry.route));
        }
        while let Some(route_id) = stack.pop() {
            traced += 1;
            if traced % 4096 == 0 && deadline_hit(deadline) {
                report.timed_out = true;
                break;
            }
            if seen_route[route_id] {
                continue;
            }
            seen_route[route_id] = true;
            let route = &routes[route_id];
            let index = local[route.value];
            if index != usize::MAX {
                selected_value[index] = true;
            }
            if let Some(producer) = route.producer {
                let index = local[producer];
                if index != usize::MAX {
                    selected_constructor[index] = true;
                }
            }
            stack.extend(route.children.iter().copied());
        }
        if report.timed_out {
            for &vertex in &part.vertices {
                local[vertex] = usize::MAX;
            }
            break;
        }

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
            }
        }
        report.processed += 1;
        for &vertex in &part.vertices {
            local[vertex] = usize::MAX;
        }
    }

    report.disabled = disabled;
    report.duration = started.elapsed();
    report
}

#[allow(clippy::type_complexity)]
fn build_component(
    graph: &Graph,
    part: &components::ComponentParts,
    local: &[usize],
    enabled: &[bool],
    width: usize,
    max_supports: usize,
    deadline: Option<Instant>,
) -> Result<(Vec<Vec<Entry>>, Vec<Route>, usize), Abort> {
    if deadline_hit(deadline) {
        return Err(Abort::Deadline);
    }
    let len = part.vertices.len();
    let mut frontier: Vec<Vec<Entry>> = vec![Vec::new(); len];
    let mut routes: Vec<Route> = Vec::new();
    let mut queue: VecDeque<usize> = VecDeque::new();

    for (bit, &vertex) in part.inputs.iter().enumerate() {
        let index = local[vertex];
        if index == usize::MAX || !enabled[vertex] {
            continue;
        }
        if add_support(
            &mut frontier[index],
            singleton(width, bit),
            vertex,
            None,
            Vec::new(),
            &mut routes,
            max_supports,
        )
        .is_err()
        {
            return Err(Abort::Wide);
        }
        queue.push_back(index);
    }

    for &constructor in &part.vertices {
        if !enabled[constructor]
            || !is_pure_constructor(graph, constructor)
            || inputs(graph, constructor).next().is_some()
        {
            continue;
        }
        for output in outputs(graph, constructor) {
            let index = local[output];
            if index == usize::MAX {
                continue;
            }
            if add_support(
                &mut frontier[index],
                empty(width),
                output,
                Some(constructor),
                Vec::new(),
                &mut routes,
                max_supports,
            )
            .is_err()
            {
                return Err(Abort::Wide);
            }
            queue.push_back(index);
        }
    }

    let mut updates = 0usize;
    let mut max_frontier = frontier.iter().map(Vec::len).max().unwrap_or(0);
    while let Some(value_index) = queue.pop_front() {
        if deadline_hit(deadline) {
            return Err(Abort::Deadline);
        }
        let value = part.vertices[value_index];
        let consumers: Vec<VertexId> = consumers(graph, value).collect();
        for constructor in consumers {
            let constructor_index = local[constructor];
            if constructor_index == usize::MAX
                || !enabled[constructor]
                || !is_pure_constructor(graph, constructor)
            {
                continue;
            }
            let mut combos: Vec<(Vec<u64>, Vec<usize>)> = vec![(empty(width), Vec::new())];
            let mut usable = true;
            for input in inputs(graph, constructor) {
                if deadline_hit(deadline) {
                    return Err(Abort::Deadline);
                }
                let input_index = local[input];
                if input_index == usize::MAX || !enabled[input] || frontier[input_index].is_empty()
                {
                    usable = false;
                    break;
                }
                let input_frontier = frontier[input_index].clone();
                if combos.len().saturating_mul(input_frontier.len())
                    > MAX_COMBINATIONS.saturating_mul(8)
                {
                    return Err(Abort::Wide);
                }
                let mut next = Vec::new();
                let mut built = 0usize;
                for (support, children) in &combos {
                    for entry in &input_frontier {
                        built += 1;
                        if built % 1024 == 0 && deadline_hit(deadline) {
                            return Err(Abort::Deadline);
                        }
                        let mut merged = support.clone();
                        for (word, other) in merged.iter_mut().zip(&entry.support) {
                            *word |= *other;
                        }
                        let mut child_routes = children.clone();
                        child_routes.push(entry.route);
                        next.push((merged, child_routes));
                    }
                }
                prune_pairs(&mut next);
                if next.len() > MAX_COMBINATIONS {
                    return Err(Abort::Wide);
                }
                combos = next;
            }
            if !usable || combos.is_empty() {
                continue;
            }
            updates += 1;
            if updates > MAX_UPDATES {
                return Err(Abort::Wide);
            }
            if updates % 64 == 0 && deadline_hit(deadline) {
                return Err(Abort::Deadline);
            }
            let outputs: Vec<VertexId> = outputs(graph, constructor).collect();
            for output in outputs {
                let output_index = local[output];
                if output_index == usize::MAX {
                    continue;
                }
                let mut changed = false;
                for (support, children) in &combos {
                    match add_support(
                        &mut frontier[output_index],
                        support.clone(),
                        output,
                        Some(constructor),
                        children.clone(),
                        &mut routes,
                        max_supports,
                    ) {
                        Ok(added) => changed |= added,
                        Err(()) => return Err(Abort::Wide),
                    }
                }
                if changed {
                    max_frontier = max_frontier.max(frontier[output_index].len());
                    queue.push_back(output_index);
                }
            }
        }
    }

    if deadline_hit(deadline) {
        return Err(Abort::Deadline);
    }
    Ok((frontier, routes, max_frontier))
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

    fn run(graph: &Graph, roots: &[VertexId], max_supports: usize) -> Report {
        filter(
            graph,
            roots,
            &vec![true; graph.graph.node_count()],
            max_supports,
            None,
        )
    }

    #[test]
    fn joint_supports_are_kept_even_when_each_input_is_individually_usable() {
        let mut graph = Graph::default();
        let a = value(&mut graph);
        let b = value(&mut graph);
        let c = value(&mut graph);
        let d = value(&mut graph);
        let v = value(&mut graph);

        let p = constructor(&mut graph, &[a, b]);
        let q = constructor(&mut graph, &[a, c]);
        let r = constructor(&mut graph, &[b, d]);
        produce(&mut graph, v, p);
        produce(&mut graph, v, q);
        produce(&mut graph, v, r);

        let report = run(&graph, &[v], 16);
        assert_eq!(report.skipped_wide, 0);
        assert!(!report.disabled[p], "{{a,b}} route must stay");
        assert!(!report.disabled[q], "{{a,c}} route must stay");
        assert!(!report.disabled[r], "{{b,d}} route must stay");
    }

    #[test]
    fn dominated_supports_are_removed() {
        let mut graph = Graph::default();
        let a = value(&mut graph);
        let b = value(&mut graph);
        let v = value(&mut graph);

        let wide = constructor(&mut graph, &[a, b]);
        let narrow = constructor(&mut graph, &[a]);
        produce(&mut graph, v, wide);
        produce(&mut graph, v, narrow);

        let report = run(&graph, &[v], 16);
        assert_eq!(report.skipped_wide, 0);
        assert!(!report.disabled[narrow]);
        assert!(report.disabled[wide], "{{a,b}} is dominated by {{a}}");
    }

    #[test]
    fn a_past_deadline_skips_without_changes() {
        let mut graph = Graph::default();
        let a = value(&mut graph);
        let b = value(&mut graph);
        let v = value(&mut graph);
        let left = constructor(&mut graph, &[a]);
        let right = constructor(&mut graph, &[b]);
        produce(&mut graph, v, left);
        produce(&mut graph, v, right);

        let deadline = Instant::now()
            .checked_sub(Duration::from_secs(1))
            .expect("past instant");
        let enabled = vec![true; graph.graph.node_count()];
        let report = filter(&graph, &[v], &enabled, 16, Some(deadline));
        assert!(report.timed_out);
        assert_eq!(report.disabled_total, 0);
        assert!(!report.disabled[left] && !report.disabled[right]);
    }

    #[test]
    fn a_frontier_overflow_skips_the_component() {
        let mut graph = Graph::default();
        let a = value(&mut graph);
        let b = value(&mut graph);
        let v = value(&mut graph);

        let left = constructor(&mut graph, &[a]);
        let right = constructor(&mut graph, &[b]);
        produce(&mut graph, v, left);
        produce(&mut graph, v, right);

        let report = run(&graph, &[v], 1);
        assert_eq!(report.skipped_wide, 1);
        assert_eq!(report.disabled_total, 0);
        assert!(!report.disabled[left] && !report.disabled[right]);
    }

    #[test]
    fn a_cycle_cannot_create_a_new_minimal_support() {
        let mut graph = Graph::default();
        let a = value(&mut graph);
        let x = value(&mut graph);
        let y = value(&mut graph);

        let base = constructor(&mut graph, &[a]);
        produce(&mut graph, x, base);
        let make_y = constructor(&mut graph, &[x]);
        produce(&mut graph, y, make_y);
        let cyclic = constructor(&mut graph, &[y]);
        produce(&mut graph, x, cyclic);

        let report = run(&graph, &[x], 16);
        assert_eq!(report.skipped_wide, 0);
        assert_eq!(report.supports, 1);
        assert!(!report.disabled[base]);
    }

    #[test]
    fn a_route_is_kept_for_every_output_support() {
        let mut graph = Graph::default();
        let a = value(&mut graph);
        let b = value(&mut graph);
        let x = value(&mut graph);
        let y = value(&mut graph);
        let root = value(&mut graph);

        let make_x_a = constructor(&mut graph, &[a]);
        let make_x_b = constructor(&mut graph, &[b]);
        produce(&mut graph, x, make_x_a);
        produce(&mut graph, x, make_x_b);
        let make_y_a = constructor(&mut graph, &[a]);
        let make_y_b = constructor(&mut graph, &[b]);
        produce(&mut graph, y, make_y_a);
        produce(&mut graph, y, make_y_b);
        let root_c = constructor(&mut graph, &[x, y]);
        produce(&mut graph, root, root_c);

        let report = run(&graph, &[root], 16);
        assert_eq!(report.skipped_wide, 0);
        assert!(!report.disabled[make_x_a] && !report.disabled[make_x_b]);
        assert!(!report.disabled[make_y_a] && !report.disabled[make_y_b]);
        assert!(!report.disabled[root_c]);
    }

    fn derivable(
        graph: &Graph,
        root: VertexId,
        enabled: &[bool],
        input_bits: &[Option<usize>],
        allowed: u64,
    ) -> bool {
        let count = graph.graph.node_count();
        let mut available = vec![false; count];
        let mut missing = vec![0u32; count];
        let mut queue = VecDeque::new();
        for (vertex, bit) in input_bits.iter().enumerate() {
            if let Some(bit) = bit {
                if allowed & (1u64 << bit) != 0 {
                    available[vertex] = true;
                }
            }
        }
        for constructor in 0..count {
            if !graph.graph[NodeIndex::new(constructor)].is_all
                || !enabled[constructor]
                || !is_pure_constructor(graph, constructor)
            {
                continue;
            }
            let pending = inputs(graph, constructor)
                .filter(|&input| !available[input])
                .count() as u32;
            missing[constructor] = pending;
            if pending == 0 {
                available[constructor] = true;
                queue.push_back(constructor);
            }
        }
        while let Some(vertex) = queue.pop_front() {
            if is_value(graph, vertex) {
                for consumer in consumers(graph, vertex) {
                    if !enabled[consumer]
                        || !is_pure_constructor(graph, consumer)
                        || available[consumer]
                        || missing[consumer] == 0
                    {
                        continue;
                    }
                    missing[consumer] -= 1;
                    if missing[consumer] == 0 {
                        available[consumer] = true;
                        queue.push_back(consumer);
                    }
                }
            } else {
                for output in outputs(graph, vertex) {
                    if enabled[output] && !available[output] {
                        available[output] = true;
                        queue.push_back(output);
                    }
                }
            }
        }
        available[root]
    }

    #[test]
    fn random_supports_are_preserved() {
        use rand::{Rng, SeedableRng, seq::IndexedRandom};

        for seed in 0..128u64 {
            let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
            let value_count = rng.random_range(4..=8);
            let input_count = rng.random_range(1..=3).min(value_count - 1);
            let mut graph = Graph::default();
            let mut values = Vec::new();
            for _ in 0..value_count {
                values.push(value(&mut graph));
            }
            for (index, &value) in values.iter().enumerate() {
                if index < input_count {
                    continue;
                }
                for _ in 0..rng.random_range(1..=3) {
                    let arity = rng.random_range(1..=3);
                    let inputs: Vec<VertexId> = (0..arity)
                        .map(|_| *values.choose(&mut rng).expect("a value"))
                        .collect();
                    let constructor = constructor(&mut graph, &inputs);
                    produce(&mut graph, value, constructor);
                }
            }
            let root = *values[input_count..]
                .choose(&mut rng)
                .expect("a non-input root");
            let report = run(&graph, &[root], 16);
            let filtered: Vec<bool> = report.disabled.iter().map(|&disabled| !disabled).collect();
            let enabled = vec![true; graph.graph.node_count()];
            let mut input_bits = vec![None; graph.graph.node_count()];
            for (bit, &input) in values[..input_count].iter().enumerate() {
                input_bits[input] = Some(bit);
            }
            for allowed in 0..(1u64 << input_count) {
                if derivable(&graph, root, &enabled, &input_bits, allowed) {
                    if !derivable(&graph, root, &filtered, &input_bits, allowed) {
                        eprintln!(
                            "seed {seed}, root {root}, inputs {:?}, allowed {allowed:#b}",
                            &values[..input_count]
                        );
                        for node in graph.graph.node_indices() {
                            eprintln!(
                                "{} all={} inputs={:?} enabled={}",
                                node.index(),
                                graph.graph[node].is_all,
                                inputs(&graph, node.index()).collect::<Vec<_>>(),
                                filtered[node.index()]
                            );
                        }
                        eprintln!(
                            "skipped_wide={} skipped_missing={} disabled={:?}",
                            report.skipped_wide, report.skipped_missing, report.disabled
                        );
                        panic!("seed {seed}, allowed {allowed:#b}: support lost");
                    }
                }
            }
        }
    }
}
