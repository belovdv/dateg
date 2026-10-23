//! Fast best-effort validity: find a valid assignment, ignore cost.
//!
//! The family shares one public configuration ([`BestEffortConfig`]) and one
//! entry point ([`solve`]), like a subcommand. [`BestEffortAlgorithm::Chain`]
//! walks the consumption chain greedily with bounded rollbacks. A shape check
//! failure or an exhausted budget returns `Unsolved`; the exact solvers are the
//! next stage. The directory is the place for future probes of this kind.
//!
//! Heuristics only: a returned assignment is always valid, but a failure means
//! "not found", not "infeasible".

use std::time::Instant;

use super::Graph;
use crate::dag::{ExtractError, StatsSink};

mod chain;

/// Which best-effort algorithm to run.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BestEffortAlgorithm {
    /// Greedy chain walk with bounded rollbacks.
    #[default]
    Chain,
}

/// Configuration for the best-effort solver family.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BestEffortConfig {
    pub algorithm: BestEffortAlgorithm,
}

impl BestEffortConfig {
    pub fn new(algorithm: BestEffortAlgorithm) -> Self {
        Self { algorithm }
    }
}

pub(super) fn solve(
    graph: &Graph,
    config: &BestEffortConfig,
    deadline: Option<Instant>,
    stats: &mut StatsSink<'_>,
) -> Result<Vec<bool>, ExtractError> {
    match config.algorithm {
        BestEffortAlgorithm::Chain => chain::solve(graph, deadline, stats),
    }
}
