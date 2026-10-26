//! Fast best-effort validity: find a valid assignment, ignore cost.
//!
//! The family shares one public configuration ([`BestEffortConfig`]) and one
//! entry point ([`solve`]), like a subcommand. [`BestEffortAlgorithm::Chain`]
//! walks the consumption chain greedily with bounded rollbacks. A shape check
//! failure or an exhausted budget returns `NotFound`; the exact solvers are the
//! next stage. The directory is the place for future probes of this kind.
//!
//! Heuristics only: a returned assignment is always valid, but a failure means
//! "not found", not "infeasible".

use std::time::Instant;

use super::{BestEffortAlgorithm, BestEffortConfig, Graph, VertexId, View};
use crate::dag::{Error, StatsSink};

mod chain;

pub(super) fn solve(
    graph: &Graph,
    roots: &[VertexId],
    view: &View,
    config: &BestEffortConfig,
    deadline: Option<Instant>,
    soft: Option<Instant>,
    stats: &mut StatsSink<'_>,
) -> Result<Vec<bool>, Error> {
    match config.algorithm {
        BestEffortAlgorithm::Chain => chain::solve(graph, roots, view, deadline, soft, stats),
    }
}
