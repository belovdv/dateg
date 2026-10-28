//! Cheap graph reductions applied before solving.
//!
//! The reductions only remove alternatives; vertex ids stay stable and removed
//! vertices are excluded in the materialized view. This keeps the reduced model
//! a model of the full graph: an assignment on the reduced view is valid on the
//! full graph as long as disabled vertices are unselected.
//!
//! Modes:
//! - structural: self-referential alternatives, equal input sets, strict subset;
//! - `DeadVertices`: root-reachability plus structural-producibility cleanup;
//! - `ComponentOptimal`: dead cleanup, structural rules, top-down component
//!   filter;
//! - `ComponentBottomUp`: dead cleanup plus one recovered route per pure
//!   component. This mode is lossy; its report sets `lossy`.
//!
//! Consumers are never removed by the structural subset rules, so the
//! consumption assignment space is preserved for later phases.

use std::time::Instant;

use petgraph::{Direction::Outgoing, graph::NodeIndex};

use super::{
    FilterMode, Graph, VertexId, bottom_up, component_filter, dead_vertices, domination,
    support_frontier,
};

#[derive(Debug)]
pub(crate) struct FilterReport {
    pub(crate) disabled: Vec<bool>,
    pub(crate) removed_self: u64,
    pub(crate) removed_duplicate: u64,
    pub(crate) removed_dominated: u64,
    pub(crate) alternatives: u64,
    pub(crate) consumers: u64,
    pub(crate) component: Option<component_filter::Report>,
    pub(crate) dead: Option<dead_vertices::Report>,
    pub(crate) bottom_up: Option<bottom_up::Report>,
    pub(crate) frontier: Option<support_frontier::Report>,
    /// The reduction may have removed the only realizable route through opaque
    /// inputs; a later `Infeasible` is not a global proof.
    pub(crate) lossy: bool,
    /// A dead root proves that no valid assignment exists.
    pub(crate) infeasible: bool,
}

impl FilterReport {
    fn new(graph: &Graph) -> Self {
        Self {
            disabled: vec![false; graph.graph.node_count()],
            removed_self: 0,
            removed_duplicate: 0,
            removed_dominated: 0,
            alternatives: 0,
            consumers: 0,
            component: None,
            dead: None,
            bottom_up: None,
            frontier: None,
            lossy: false,
            infeasible: false,
        }
    }
}

fn merge(enabled: &mut [bool], disabled: &mut [bool], removed: &[bool]) {
    for (index, &removed) in removed.iter().enumerate() {
        if removed {
            enabled[index] = false;
            disabled[index] = true;
        }
    }
}

#[cfg(test)]
pub(crate) fn filter(graph: &Graph, mode: FilterMode) -> FilterReport {
    filter_with_roots(
        graph,
        mode,
        &vec![true; graph.graph.node_count()],
        &[],
        None,
    )
}

pub(crate) fn filter_with_roots(
    graph: &Graph,
    mode: FilterMode,
    enabled: &[bool],
    roots: &[VertexId],
    deadline: Option<Instant>,
) -> FilterReport {
    match mode {
        FilterMode::ComponentOptimal => component_optimal(graph, enabled, roots, deadline),
        FilterMode::DeadVertices => dead_only(graph, enabled, roots, deadline),
        FilterMode::ComponentBottomUp => component_bottom_up(graph, enabled, roots),
        FilterMode::ComponentBottomUpFrontier(max_supports) => {
            component_frontier(graph, enabled, roots, max_supports, deadline)
        }
        FilterMode::AtLeastOneOptimal | FilterMode::AtLeastOneSolution => {
            structural_filter(graph, mode, enabled)
        }
    }
}

fn dead_only(
    graph: &Graph,
    enabled: &[bool],
    roots: &[VertexId],
    deadline: Option<Instant>,
) -> FilterReport {
    let dead = dead_vertices::filter(graph, roots, enabled, deadline);
    let mut report = FilterReport::new(graph);
    let mut enabled = enabled.to_vec();
    merge(&mut enabled, &mut report.disabled, &dead.disabled);
    report.infeasible = dead.root_dead;
    report.dead = Some(dead);
    report
}

fn component_optimal(
    graph: &Graph,
    enabled: &[bool],
    roots: &[VertexId],
    deadline: Option<Instant>,
) -> FilterReport {
    let dead = dead_vertices::filter(graph, roots, enabled, deadline);
    let mut report = FilterReport::new(graph);
    let mut enabled = enabled.to_vec();
    merge(&mut enabled, &mut report.disabled, &dead.disabled);
    report.infeasible = dead.root_dead;
    report.dead = Some(dead);
    if report.infeasible {
        return report;
    }

    let structural = structural_filter(graph, FilterMode::AtLeastOneOptimal, &enabled);
    merge(&mut enabled, &mut report.disabled, &structural.disabled);
    report.removed_self = structural.removed_self;
    report.removed_duplicate = structural.removed_duplicate;
    report.removed_dominated = structural.removed_dominated;
    report.alternatives = structural.alternatives;
    report.consumers = structural.consumers;

    let filtered = component_filter::filter(graph, roots, &enabled, deadline, Default::default());
    merge(&mut enabled, &mut report.disabled, &filtered.disabled);
    report.component = Some(filtered);
    report
}

fn component_bottom_up(graph: &Graph, enabled: &[bool], roots: &[VertexId]) -> FilterReport {
    // The pass is a last resort: it ignores the pipeline deadline because it is
    // O(V+E) and must still run after the exact stages exhausted the budget.
    let dead = dead_vertices::filter(graph, roots, enabled, None);
    let mut report = FilterReport::new(graph);
    let mut enabled = enabled.to_vec();
    merge(&mut enabled, &mut report.disabled, &dead.disabled);
    report.infeasible = dead.root_dead;
    if !dead.root_dead {
        let filtered = bottom_up::filter(graph, roots, &enabled, 1);
        merge(&mut enabled, &mut report.disabled, &filtered.disabled);
        report.lossy = filtered.disabled_total > 0;
        report.bottom_up = Some(filtered);
    }
    report.dead = Some(dead);
    report
}

fn component_frontier(
    graph: &Graph,
    enabled: &[bool],
    roots: &[VertexId],
    max_supports: usize,
    deadline: Option<Instant>,
) -> FilterReport {
    let dead = dead_vertices::filter(graph, roots, enabled, deadline);
    let mut report = FilterReport::new(graph);
    let mut enabled = enabled.to_vec();
    merge(&mut enabled, &mut report.disabled, &dead.disabled);
    report.infeasible = dead.root_dead;
    if !dead.root_dead {
        let filtered = support_frontier::filter(graph, roots, &enabled, max_supports, deadline);
        merge(&mut enabled, &mut report.disabled, &filtered.disabled);
        report.frontier = Some(filtered);
    }
    report.dead = Some(dead);
    report
}

fn structural_filter(graph: &Graph, mode: FilterMode, enabled: &[bool]) -> FilterReport {
    let mut report = FilterReport::new(graph);
    let mut removed_self = 0;
    let mut removed_duplicate = 0;
    let mut removed_dominated = 0;
    let mut alternatives = 0;
    let mut consumers = 0;

    for node in graph.graph.node_indices() {
        if !graph.graph[node].is_all {
            continue;
        }
        alternatives += 1;
        let constructor = node.index();
        if domination::is_consumer(graph, constructor) {
            consumers += 1;
        }
        let Some(class) = domination::output_class(graph, constructor) else {
            continue;
        };
        if domination::input_set(graph, constructor)
            .binary_search(&class)
            .is_ok()
            && enabled[constructor]
        {
            report.disabled[constructor] = true;
            removed_self += 1;
        }
    }

    for class in graph.graph.node_indices() {
        if graph.graph[class].is_all {
            continue;
        }
        let mut candidates: Vec<VertexId> = graph
            .graph
            .neighbors_directed(class, Outgoing)
            .map(|next| next.index())
            .filter(|&candidate| {
                enabled[candidate]
                    && !report.disabled[candidate]
                    && !domination::is_consumer(graph, candidate)
            })
            .collect();
        // Deterministic order: fewest inputs first, then cheapest, then id.
        candidates.sort_unstable_by_key(|&candidate| {
            (
                domination::input_set(graph, candidate).len(),
                graph.graph[NodeIndex::new(candidate)].cost,
                candidate,
            )
        });

        let mut kept: Vec<(Vec<VertexId>, usize)> = Vec::new();
        for candidate in candidates {
            let inputs = domination::input_set(graph, candidate);
            let cost = graph.graph[NodeIndex::new(candidate)].cost;
            let dominated = kept.iter().any(|(other_inputs, other_cost)| {
                domination::is_subset(other_inputs, &inputs)
                    && (mode == FilterMode::AtLeastOneSolution || *other_cost <= cost)
            });
            if !dominated {
                kept.push((inputs, cost));
                continue;
            }
            let duplicate = kept.iter().any(|(other_inputs, _)| other_inputs == &inputs);
            report.disabled[candidate] = true;
            if duplicate {
                removed_duplicate += 1;
            } else {
                removed_dominated += 1;
            }
        }
    }

    report.removed_self = removed_self;
    report.removed_duplicate = removed_duplicate;
    report.removed_dominated = removed_dominated;
    report.alternatives = alternatives;
    report.consumers = consumers;
    report
}
