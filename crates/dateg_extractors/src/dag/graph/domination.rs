//! Structural relations between alternative constructors.
//!
//! The reductions in [`super::filtration`] only need local facts: a
//! constructor's input classes, its output class, and whether it consumes
//! anything. Requirement/support analysis is deliberately not here yet.

use petgraph::{
    Direction::{Incoming, Outgoing},
    graph::NodeIndex,
};

use super::{Graph, VertexId};

/// Input classes of a constructor, as a sorted duplicate-free set.
///
/// Edges are stored as a set (`update_edge`), so duplicate input positions are
/// already collapsed; sorting and deduping keeps the comparison explicit.
pub(super) fn input_set(graph: &Graph, constructor: VertexId) -> Vec<VertexId> {
    let mut inputs: Vec<_> = graph
        .graph
        .neighbors_directed(NodeIndex::new(constructor), Outgoing)
        .map(|next| next.index())
        .collect();
    inputs.sort_unstable();
    inputs.dedup();
    inputs
}

/// The class this constructor is an alternative of.
pub(super) fn output_class(graph: &Graph, constructor: VertexId) -> Option<VertexId> {
    graph
        .graph
        .neighbors_directed(NodeIndex::new(constructor), Incoming)
        .find(|&next| !graph.graph[next].is_all)
        .map(|next| next.index())
}

pub(super) fn is_consumer(graph: &Graph, constructor: VertexId) -> bool {
    !graph.graph[NodeIndex::new(constructor)].consumes.is_empty()
}

/// `left ⊆ right` for sorted slices.
pub(super) fn is_subset(left: &[VertexId], right: &[VertexId]) -> bool {
    if left.len() > right.len() {
        return false;
    }
    left.iter().all(|item| right.binary_search(item).is_ok())
}
