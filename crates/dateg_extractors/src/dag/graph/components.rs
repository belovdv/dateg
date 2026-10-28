//! Pure-component decomposition.
//!
//! A component is a maximal connected set `V` of values and pure constructors.
//! Its interface `I` is the part of `V` whose dependencies leave `V`; those
//! dependencies may be impure. Only operations inside `V` have to be pure.
//! `U` is the part of `V` used from outside. `C` is not materialized here:
//! the filter treats nullary pure constructors as constants.
//!
//! For every `v` in `V`, `f(v)` is the set of inputs that every way to
//! construct `v` uses. It is computed as a greatest fixpoint: union over the
//! inputs of a constructor, intersection over the producers of a value.
//! Vertices without a finite acyclic derivation are reported as unfounded.
//!
//! The decomposition is the reusable entry point shared by the top-down
//! component filter and the bottom-up filter (`decompose` returns the light
//! parts; `analyze` adds the required-input fixpoint).

#![allow(dead_code)] // component data is shared with future filters

use std::collections::VecDeque;

use petgraph::{
    Direction::{Incoming, Outgoing},
    graph::NodeIndex,
};

use super::{Graph, VertexId};

/// One maximal connected pure component.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Component {
    /// Sorted vertex set `V`.
    pub(crate) vertices: Vec<VertexId>,
    /// Sorted `I`: values in `V` with a dependency outside `V`, plus values
    /// with no producer at all.
    pub(crate) inputs: Vec<VertexId>,
    /// Inputs that also have a pure producer inside `V`; production could
    /// continue, but the external dependency still makes them the cut.
    pub(crate) shared_inputs: Vec<VertexId>,
    /// Sorted `U`: vertices in `V` used from outside `V`.
    pub(crate) outputs: Vec<VertexId>,
    /// `v -> required inputs f(v)` for every producible vertex of `V`.
    required_inputs: Vec<Option<Vec<VertexId>>>,
    /// Vertices of `V` without a finite acyclic derivation.
    pub(crate) unfounded: Vec<VertexId>,
    pub(crate) violations: Vec<String>,
}

impl Component {
    /// The required inputs of `v`, or `None` when `v` is unfounded.
    pub(crate) fn required_inputs(&self, vertex: VertexId) -> Option<&[VertexId]> {
        self.required_inputs[vertex].as_deref()
    }
}

fn is_value(graph: &Graph, vertex: VertexId) -> bool {
    !graph.graph[NodeIndex::new(vertex)].is_all
}

fn is_pure_constructor(graph: &Graph, vertex: VertexId) -> bool {
    let node = &graph.graph[NodeIndex::new(vertex)];
    node.is_all && node.consumes.is_empty()
}

fn inputs_of(graph: &Graph, constructor: VertexId) -> impl Iterator<Item = VertexId> + '_ {
    graph
        .graph
        .neighbors_directed(NodeIndex::new(constructor), Outgoing)
        .map(|node| node.index())
}

fn producers_of(graph: &Graph, value: VertexId) -> impl Iterator<Item = VertexId> + '_ {
    graph
        .graph
        .neighbors_directed(NodeIndex::new(value), Outgoing)
        .map(|node| node.index())
}

fn output_values(graph: &Graph, constructor: VertexId) -> impl Iterator<Item = VertexId> + '_ {
    graph
        .graph
        .neighbors_directed(NodeIndex::new(constructor), Incoming)
        .map(|node| node.index())
}

fn consumers_of(graph: &Graph, value: VertexId) -> impl Iterator<Item = VertexId> + '_ {
    graph
        .graph
        .neighbors_directed(NodeIndex::new(value), Incoming)
        .map(|node| node.index())
}

fn is_region_input(graph: &Graph, inside: &[bool], vertex: VertexId) -> bool {
    if !is_value(graph, vertex) {
        return false;
    }
    let mut producers = producers_of(graph, vertex).peekable();
    producers.peek().is_none() || producers.any(|producer| !inside[producer])
}

/// Connected components of the graph where edges are incident to a pure
/// constructor. Impure constructors are never part of a component.
fn pure_components(graph: &Graph) -> Vec<Vec<VertexId>> {
    let count = graph.graph.node_count();
    let mut adjacency = vec![Vec::new(); count];
    for constructor in 0..count {
        if !is_pure_constructor(graph, constructor) {
            continue;
        }
        for value in inputs_of(graph, constructor).chain(output_values(graph, constructor)) {
            adjacency[constructor].push(value);
            adjacency[value].push(constructor);
        }
    }

    let mut seen = vec![false; count];
    let mut components = Vec::new();
    for start in 0..count {
        if seen[start] || adjacency[start].is_empty() {
            continue;
        }
        let mut component = Vec::new();
        let mut stack = vec![start];
        seen[start] = true;
        while let Some(vertex) = stack.pop() {
            component.push(vertex);
            for &next in &adjacency[vertex] {
                if !seen[next] {
                    seen[next] = true;
                    stack.push(next);
                }
            }
        }
        component.sort_unstable();
        components.push(component);
    }
    components.sort_unstable_by_key(|component| (usize::MAX - component.len(), component[0]));
    components
}

fn full_mask(width: usize, inputs: usize) -> Vec<u64> {
    let mut mask = vec![u64::MAX; width];
    if width > 0 {
        let unused = width * 64 - inputs;
        if unused > 0 {
            mask[width - 1] = u64::MAX >> unused;
        }
    }
    mask
}

fn subset(left: &[u64], right: &[u64]) -> bool {
    left.iter()
        .zip(right)
        .all(|(left, right)| left & !right == 0)
}

fn mask_ids(mask: &[u64], inputs: &[VertexId]) -> Vec<VertexId> {
    let mut ids = Vec::new();
    for (word_index, &word) in mask.iter().enumerate() {
        let mut word = word;
        while word != 0 {
            let bit = word.trailing_zeros() as usize;
            ids.push(inputs[word_index * 64 + bit]);
            word &= word - 1;
        }
    }
    ids
}

fn region_required_inputs(
    graph: &Graph,
    vertices: &[VertexId],
    inside: &[bool],
    inputs: &[VertexId],
    unfounded: &mut Vec<VertexId>,
) -> (Vec<Option<Vec<VertexId>>>, Vec<String>) {
    let count = graph.graph.node_count();
    let width = inputs.len().div_ceil(64);
    let max = full_mask(width, inputs.len());
    let mut slots = vec![None; count];
    for (slot, &input) in inputs.iter().enumerate() {
        slots[input] = Some(slot);
    }

    let mut masks = vec![Vec::new(); count];
    for &vertex in vertices {
        masks[vertex] = max.clone();
    }
    for (slot, &input) in inputs.iter().enumerate() {
        masks[input].fill(0);
        masks[input][slot / 64] |= 1u64 << (slot % 64);
    }

    let mut queue: VecDeque<VertexId> = vertices.iter().copied().collect();
    let mut queued = vec![false; count];
    for &vertex in vertices {
        queued[vertex] = true;
    }
    while let Some(vertex) = queue.pop_front() {
        queued[vertex] = false;
        if slots[vertex].is_some() {
            continue;
        }
        let mut next = if is_value(graph, vertex) {
            let mut next = max.clone();
            for producer in producers_of(graph, vertex) {
                for (dst, src) in next.iter_mut().zip(&masks[producer]) {
                    *dst &= *src;
                }
            }
            next
        } else {
            let mut next = vec![0u64; width];
            for input in inputs_of(graph, vertex) {
                for (dst, src) in next.iter_mut().zip(&masks[input]) {
                    *dst |= *src;
                }
            }
            next
        };
        next.iter_mut()
            .zip(&max)
            .for_each(|(word, mask)| *word &= *mask);
        if next != masks[vertex] {
            masks[vertex] = next;
            for dependent in consumers_of(graph, vertex) {
                if inside[dependent] && !queued[dependent] {
                    queued[dependent] = true;
                    queue.push_back(dependent);
                }
            }
        }
    }

    let mut violations = Vec::new();
    let mut required_inputs = vec![None; count];
    for &vertex in vertices {
        let mask = &masks[vertex];
        required_inputs[vertex] = Some(mask_ids(mask, inputs));
    }

    // Producibility (finite acyclic derivations), used to separate unfounded
    // vertices from the universal fixpoint value.
    let mut producible = vec![false; count];
    let mut missing = vec![0usize; count];
    let mut queue: VecDeque<VertexId> = VecDeque::new();
    for &vertex in vertices {
        if slots[vertex].is_some() {
            producible[vertex] = true;
            queue.push_back(vertex);
        } else if !is_value(graph, vertex) {
            missing[vertex] = inputs_of(graph, vertex).count();
            if missing[vertex] == 0 {
                producible[vertex] = true;
                for output in output_values(graph, vertex) {
                    if !producible[output] {
                        producible[output] = true;
                        queue.push_back(output);
                    }
                }
            }
        }
    }
    while let Some(vertex) = queue.pop_front() {
        if !is_value(graph, vertex) {
            continue;
        }
        for consumer in consumers_of(graph, vertex) {
            if !inside[consumer] || producible[consumer] {
                continue;
            }
            missing[consumer] = missing[consumer].saturating_sub(1);
            if missing[consumer] == 0 {
                producible[consumer] = true;
                for output in output_values(graph, consumer) {
                    if !producible[output] {
                        producible[output] = true;
                        queue.push_back(output);
                    }
                }
            }
        }
    }

    for &vertex in vertices {
        if !producible[vertex] {
            unfounded.push(vertex);
            required_inputs[vertex] = None;
        }
    }

    // Structural checks: every in-region operation is pure; every non-input
    // dependency is inside; every way uses all of the vertex required inputs.
    for &vertex in vertices {
        if !is_value(graph, vertex) && !is_pure_constructor(graph, vertex) {
            violations.push(format!("impure operation {vertex} is inside the region"));
        }
        if slots[vertex].is_some() {
            continue;
        }
        for next in producers_of(graph, vertex) {
            if !inside[next] {
                violations.push(format!("{vertex} depends on {next} outside the region"));
            }
        }
        if is_value(graph, vertex) {
            let required = &masks[vertex];
            for producer in producers_of(graph, vertex) {
                if inside[producer]
                    && required_inputs[producer].is_some()
                    && !subset(required, &masks[producer])
                {
                    violations.push(format!(
                        "way {producer} does not use all required inputs of {vertex}"
                    ));
                }
            }
        } else {
            for input in inputs_of(graph, vertex) {
                if inside[input]
                    && required_inputs[input].is_some()
                    && !subset(&masks[input], &masks[vertex])
                {
                    violations.push(format!("required input {input} is not used by {vertex}"));
                }
            }
        }
    }

    (required_inputs, violations)
}

/// Component decomposition without the required-input fixpoint. Used by
/// reductions that only need the `V`/`I`/`U` sets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ComponentParts {
    /// Sorted vertex set `V`.
    pub(crate) vertices: Vec<VertexId>,
    /// Sorted `I`.
    pub(crate) inputs: Vec<VertexId>,
    /// Inputs that also have a pure producer inside `V`.
    pub(crate) shared_inputs: Vec<VertexId>,
    /// Sorted `U`.
    pub(crate) outputs: Vec<VertexId>,
}

fn component_parts(graph: &Graph, vertices: Vec<VertexId>) -> ComponentParts {
    let count = graph.graph.node_count();
    let mut inside = vec![false; count];
    for &vertex in &vertices {
        inside[vertex] = true;
    }
    let inputs: Vec<_> = vertices
        .iter()
        .copied()
        .filter(|&vertex| is_region_input(graph, &inside, vertex))
        .collect();
    let shared_inputs: Vec<_> = inputs
        .iter()
        .copied()
        .filter(|&vertex| producers_of(graph, vertex).any(|producer| inside[producer]))
        .collect();
    let outputs = vertices
        .iter()
        .copied()
        .filter(|&vertex| {
            !is_value(graph, vertex) || consumers_of(graph, vertex).any(|c| !inside[c])
        })
        .filter(|&vertex| is_value(graph, vertex))
        .collect();
    ComponentParts {
        vertices,
        inputs,
        shared_inputs,
        outputs,
    }
}

pub(crate) fn decompose(graph: &Graph) -> Vec<ComponentParts> {
    pure_components(graph)
        .into_iter()
        .map(|vertices| component_parts(graph, vertices))
        .collect()
}

fn build_region(graph: &Graph, parts: ComponentParts) -> Component {
    let count = graph.graph.node_count();
    let mut inside = vec![false; count];
    for &vertex in &parts.vertices {
        inside[vertex] = true;
    }
    let ComponentParts {
        vertices,
        inputs,
        shared_inputs,
        outputs,
    } = parts;

    let mut violations = Vec::new();
    for &vertex in &vertices {
        if inputs.binary_search(&vertex).is_ok() {
            continue;
        }
        for next in producers_of(graph, vertex) {
            if !inside[next] {
                violations.push(format!("{vertex} depends on {next} outside the region"));
            }
        }
    }

    let mut unfounded = Vec::new();
    let (required_inputs, required_violations) =
        region_required_inputs(graph, &vertices, &inside, &inputs, &mut unfounded);
    violations.extend(required_violations);

    Component {
        vertices,
        inputs,
        shared_inputs,
        outputs,
        required_inputs,
        unfounded,
        violations,
    }
}

pub(crate) fn analyze(graph: &Graph) -> Vec<Component> {
    decompose(graph)
        .into_iter()
        .map(|parts| build_region(graph, parts))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Expr {
        a: VertexId,
        b: VertexId,
        c: VertexId,
        d: VertexId,
        ab: VertexId,
        abc: VertexId,
        neg_d: VertexId,
        root: VertexId,
    }

    fn value(graph: &mut Graph) -> VertexId {
        graph.add_vertex(false, 0, || None)
    }

    fn constructor(graph: &mut Graph, inputs: &[VertexId]) -> VertexId {
        let constructor = graph.add_vertex(true, 1, || None);
        graph.add_edges(constructor, inputs.iter().copied());
        constructor
    }

    fn input(graph: &mut Graph, memory: VertexId) -> VertexId {
        let value = value(graph);
        let load = graph.add_vertex(true, 1, || None);
        graph.set_consumes(load, [memory]);
        graph.add_edges(value, [load]);
        value
    }

    fn output(graph: &mut Graph, memory: VertexId, value: VertexId) {
        let store = graph.add_vertex(true, 1, || None);
        graph.set_consumes(store, [memory]);
        graph.add_edges(store, [value]);
    }

    fn expression() -> (Graph, Expr) {
        let mut graph = Graph::default();
        let memory = value(&mut graph);
        let a = input(&mut graph, memory);
        let b = input(&mut graph, memory);
        let c = input(&mut graph, memory);
        let d = input(&mut graph, memory);

        let ab = value(&mut graph);
        let add_ab = constructor(&mut graph, &[a, b]);
        graph.add_edges(ab, [add_ab]);

        let abc = value(&mut graph);
        let add_abc = constructor(&mut graph, &[ab, c]);
        graph.add_edges(abc, [add_abc]);

        let neg_d = value(&mut graph);
        let neg = constructor(&mut graph, &[d]);
        graph.add_edges(neg_d, [neg]);

        let root = value(&mut graph);
        let sub = constructor(&mut graph, &[abc, neg_d]);
        graph.add_edges(root, [sub]);
        output(&mut graph, memory, root);

        (
            graph,
            Expr {
                a,
                b,
                c,
                d,
                ab,
                abc,
                neg_d,
                root,
            },
        )
    }

    #[test]
    fn a_plus_b_plus_c_minus_d_has_the_four_inputs() {
        let (graph, expr) = expression();
        let components = analyze(&graph);
        assert_eq!(components.len(), 1);
        let region = &components[0];
        assert_eq!(region.inputs, vec![expr.a, expr.b, expr.c, expr.d]);
        assert_eq!(region.outputs, vec![expr.root]);
        assert!(region.unfounded.is_empty());
        assert_eq!(
            region.required_inputs(expr.root),
            Some([expr.a, expr.b, expr.c, expr.d].as_slice())
        );
        assert_eq!(
            region.required_inputs(expr.abc),
            Some([expr.a, expr.b, expr.c].as_slice())
        );
        assert_eq!(
            region.required_inputs(expr.ab),
            Some([expr.a, expr.b].as_slice())
        );
        assert_eq!(
            region.required_inputs(expr.neg_d),
            Some([expr.d].as_slice())
        );
        assert!(
            region.violations.is_empty(),
            "violations: {:?}",
            region.violations
        );
    }

    #[test]
    fn alternatives_intersect_their_required_inputs() {
        let mut graph = Graph::default();
        let memory = value(&mut graph);
        let a = input(&mut graph, memory);
        let b = input(&mut graph, memory);
        let c = input(&mut graph, memory);

        let x = value(&mut graph);
        let left = constructor(&mut graph, &[a, b]);
        let right = constructor(&mut graph, &[a, c]);
        graph.add_edges(x, [left]);
        graph.add_edges(x, [right]);

        let components = analyze(&graph);
        assert_eq!(components.len(), 1);
        let region = &components[0];
        assert_eq!(region.inputs, vec![a, b, c]);
        assert_eq!(region.required_inputs(x), Some([a].as_slice()));
        assert!(region.violations.is_empty());
    }

    #[test]
    fn cyclic_alternatives_are_reported_unfounded() {
        let mut graph = Graph::default();
        let memory = value(&mut graph);
        let a = input(&mut graph, memory);
        let b = input(&mut graph, memory);
        let c = input(&mut graph, memory);

        // x = add(y, a), y = add(x, b): no finite way to construct either.
        let x = value(&mut graph);
        let y = value(&mut graph);
        let add_y_a = constructor(&mut graph, &[y, a]);
        let add_x_b = constructor(&mut graph, &[x, b]);
        graph.add_edges(x, [add_y_a]);
        graph.add_edges(y, [add_x_b]);

        // A finite alternative for x: x = add(b, c) as well.
        let add_b_c = constructor(&mut graph, &[b, c]);
        graph.add_edges(x, [add_b_c]);

        let components = analyze(&graph);
        let region = &components[0];
        assert!(region.violations.is_empty());
        assert_eq!(region.required_inputs(x), Some([b, c].as_slice()));
        // y needs x (finite via b+c), so y becomes producible too.
        assert_eq!(region.required_inputs(y), Some([b, c].as_slice()));

        let mut graph = Graph::default();
        let memory = value(&mut graph);
        let a = input(&mut graph, memory);
        let b = input(&mut graph, memory);
        let x = value(&mut graph);
        let y = value(&mut graph);
        let add_y_a = constructor(&mut graph, &[y, a]);
        let add_x_b = constructor(&mut graph, &[x, b]);
        graph.add_edges(x, [add_y_a]);
        graph.add_edges(y, [add_x_b]);
        let components = analyze(&graph);
        let region = &components[0];
        assert_eq!(region.required_inputs(x), None);
        assert_eq!(region.required_inputs(y), None);
        assert!(region.unfounded.contains(&x) && region.unfounded.contains(&y));
    }

    #[test]
    fn maximal_regions_merge_subexpressions() {
        let mut graph = Graph::default();
        let memory = value(&mut graph);
        let a = input(&mut graph, memory);
        let b = input(&mut graph, memory);
        let c = input(&mut graph, memory);
        let d = input(&mut graph, memory);
        let e = input(&mut graph, memory);
        let f = input(&mut graph, memory);

        // a + b + c: `ab` must not become a region of its own.
        let ab = value(&mut graph);
        let add_ab = constructor(&mut graph, &[a, b]);
        graph.add_edges(ab, [add_ab]);

        let abc = value(&mut graph);
        let add_abc = constructor(&mut graph, &[ab, c]);
        graph.add_edges(abc, [add_abc]);

        // d + e + f: a separate maximal region.
        let def = value(&mut graph);
        let add_def = constructor(&mut graph, &[d, e, f]);
        graph.add_edges(def, [add_def]);
        output(&mut graph, memory, abc);
        output(&mut graph, memory, def);

        let components = analyze(&graph);
        assert_eq!(components.len(), 2);
        assert!(components.iter().all(|region| region.violations.is_empty()));

        let contains = |region: &Component, vertex: VertexId| region.vertices.contains(&vertex);
        let first = components
            .iter()
            .find(|region| contains(region, add_ab))
            .expect("the a+b region");
        assert!(contains(first, ab) && contains(first, add_abc) && contains(first, abc));
        assert!(!contains(first, add_def) && !contains(first, def));

        let second = components
            .iter()
            .find(|region| contains(region, add_def))
            .expect("the d+e+f region");
        assert!(contains(second, def));
        assert!(!contains(second, add_ab) && !contains(second, abc));

        assert_eq!(first.inputs, vec![a, b, c]);
        assert_eq!(first.required_inputs(abc), Some([a, b, c].as_slice()));
        assert_eq!(first.required_inputs(ab), Some([a, b].as_slice()));
        assert_eq!(first.outputs, vec![abc]);
        assert_eq!(second.inputs, vec![d, e, f]);
        assert_eq!(second.required_inputs(def), Some([d, e, f].as_slice()));
    }
}
