use std::time::Duration;

use super::{AtomicAlgorithm, FilterMode, Node, SolverConfig, TimeoutPolicy};

impl Node {
    /// Run all children in order, keeping the cheapest result.
    pub fn bundle(children: impl IntoIterator<Item = Node>) -> Self {
        Self::Bundle(children.into_iter().collect())
    }

    /// Apply `mode` before this node. The filter's own budget is the parent's.
    pub fn with_prefilter(self, mode: FilterMode) -> Self {
        Self::Filter {
            mode,
            budget: None,
            child: Box::new(self),
        }
    }

    /// Record a stage label for statistics.
    pub fn with_label(self, name: &'static str) -> Self {
        Self::Label {
            name,
            child: Box::new(self),
        }
    }

    /// Run this node with a child budget.
    pub fn with_timeout(self, policy: TimeoutPolicy) -> Self {
        Self::Timeout {
            policy,
            child: Box::new(self),
        }
    }

    /// Run `fallback` only when this node produced no solution.
    pub fn or_else(self, fallback: impl Into<Node>) -> Self {
        Self::Try {
            primary: Box::new(self),
            fallback: Box::new(fallback.into()),
        }
    }

    /// Run `fallback` after a failure, then continue with the parent.
    pub fn with_fallback(self, fallback: impl Into<Node>) -> Self {
        Self::OnFailure {
            primary: Box::new(self),
            fallback: Box::new(fallback.into()),
        }
    }
}

impl SolverConfig {
    pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

    pub fn new(pipeline: impl Into<Node>) -> Self {
        Self {
            pipeline: pipeline.into(),
            timeout: Self::DEFAULT_TIMEOUT,
            soft_timeout: None,
            name: None,
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn with_soft_timeout(mut self, timeout: Duration) -> Self {
        self.soft_timeout = Some(timeout);
        self
    }

    pub fn with_name(mut self, name: &'static str) -> Self {
        self.name = Some(name);
        self
    }

    /// Minimal: dead-vertex cleanup, then descending SAT only.
    pub fn fast() -> Self {
        Self::new(
            Node::from(AtomicAlgorithm::SatDescending(Default::default()))
                .with_prefilter(FilterMode::DeadVertices),
        )
    }

    /// Dead-vertex cleanup, chain UB, then descending SAT.
    pub fn balanced() -> Self {
        Self::new(
            Node::bundle([
                Node::from(AtomicAlgorithm::Chain(Default::default())).with_label("first"),
                Node::from(AtomicAlgorithm::SatDescending(Default::default())).with_label("second"),
            ])
            .with_prefilter(FilterMode::DeadVertices),
        )
    }

    /// Reliability-oriented: dead-vertex cleanup, then exact MaxSAT and SAT
    /// within 90% of the budget. On failure, spend the remainder on the
    /// validity-preserving support-frontier fallback.
    pub fn thorough() -> Self {
        Self::thorough_ext(TimeoutPolicy::Fraction(0.9), 16)
    }

    /// `thorough` with the primary-stage budget and the frontier cap exposed.
    /// The fallback gets whatever `primary` leaves.
    pub fn thorough_ext(primary: TimeoutPolicy, frontier_supports: usize) -> Self {
        Self::new(
            Node::bundle([
                Node::from(AtomicAlgorithm::Chain(Default::default())).with_label("first"),
                Node::bundle([
                    Node::from(AtomicAlgorithm::MaxSat(Default::default())).with_label("baseline"),
                    Node::from(AtomicAlgorithm::SatDescending(Default::default()))
                        .with_label("second"),
                ])
                .with_prefilter(FilterMode::AtLeastOneOptimal),
            ])
            .with_timeout(primary)
            .with_fallback(
                Node::from(AtomicAlgorithm::SatDescending(Default::default()))
                    .with_label("hammer")
                    .with_prefilter(FilterMode::ComponentBottomUpFrontier(frontier_supports)),
            )
            .with_prefilter(FilterMode::DeadVertices),
        )
    }

    /// Exact reference: dead-vertex cleanup, then MaxSAT only.
    pub fn exact() -> Self {
        Self::new(
            Node::from(AtomicAlgorithm::MaxSat(Default::default()))
                .with_prefilter(FilterMode::DeadVertices),
        )
    }
}
