//! Cheap graph reductions applied before solving.
//!
//! The reductions only remove alternatives; vertex ids stay stable and removed
//! vertices are excluded in the materialized view. This keeps the reduced model
//! a model of the full graph: an assignment on the reduced view is valid on the
//! full graph as long as disabled vertices are unselected.
//!
//! Rules (all local, no fixed points):
//! - self-referential alternative: a constructor whose own output class is one
//!   of its inputs can never be selected in an acyclic solution;
//! - equal input sets: pure alternatives that need exactly the same classes are
//!   interchangeable;
//! - strict subset: a pure alternative whose inputs are a strict subset of
//!   another's is always at least as feasible.
//!
//! Consumers are never removed by the last two rules, so the consumption
//! assignment space is preserved for later phases.

use petgraph::{Direction::Outgoing, graph::NodeIndex};

use super::{FilterMode, Graph, VertexId, domination};

#[derive(Debug)]
pub(crate) struct FilterReport {
    pub(crate) disabled: Vec<bool>,
    pub(crate) removed_self: u64,
    pub(crate) removed_duplicate: u64,
    pub(crate) removed_dominated: u64,
    pub(crate) alternatives: u64,
    pub(crate) consumers: u64,
}

pub(crate) fn filter(graph: &Graph, mode: FilterMode) -> FilterReport {
    let enabled = vec![true; graph.graph.node_count()];
    filter_with_enabled(graph, mode, &enabled)
}

/// Filter relative to an existing view. Already disabled constructors are not
/// candidates and cannot act as dominators.
pub(crate) fn filter_with_enabled(
    graph: &Graph,
    mode: FilterMode,
    enabled: &[bool],
) -> FilterReport {
    let count = graph.graph.node_count();
    let mut disabled = vec![false; count];
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
            disabled[constructor] = true;
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
                    && !disabled[candidate]
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
            disabled[candidate] = true;
            if duplicate {
                removed_duplicate += 1;
            } else {
                removed_dominated += 1;
            }
        }
    }

    FilterReport {
        disabled,
        removed_self,
        removed_duplicate,
        removed_dominated,
        alternatives,
        consumers,
    }
}
