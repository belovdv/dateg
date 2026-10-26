use std::time::Duration;

use super::{AtomicAlgorithm, FilterMode, Node, SolverConfig};

impl SolverConfig {
    pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

    pub fn new(pipeline: Node) -> Self {
        Self {
            pipeline,
            timeout: Some(Self::DEFAULT_TIMEOUT),
            soft_timeout: None,
            name: None,
        }
    }

    /// Minimal: descending SAT only.
    pub fn fast() -> Self {
        Self::new(Node::Atomic(AtomicAlgorithm::SatDescending(
            Default::default(),
        )))
    }

    /// Chain UB first, then descending SAT.
    pub fn balanced() -> Self {
        Self::new(Node::Bundle(vec![
            Node::Label {
                name: "first",
                child: Box::new(Node::Atomic(AtomicAlgorithm::Chain(Default::default()))),
            },
            Node::Label {
                name: "second",
                child: Box::new(Node::Atomic(AtomicAlgorithm::SatDescending(
                    Default::default(),
                ))),
            },
        ]))
    }

    /// Reliability-oriented: chain, structural filter, exact MaxSAT, then SAT.
    /// Cost-bound probes are added with the pruner.
    pub fn thorough() -> Self {
        Self::new(Node::Bundle(vec![
            Node::Label {
                name: "first",
                child: Box::new(Node::Atomic(AtomicAlgorithm::Chain(Default::default()))),
            },
            Node::Filter(FilterMode::AtLeastOneOptimal),
            Node::Label {
                name: "baseline",
                child: Box::new(Node::Atomic(AtomicAlgorithm::MaxSat(Default::default()))),
            },
            Node::Label {
                name: "second",
                child: Box::new(Node::Atomic(AtomicAlgorithm::SatDescending(
                    Default::default(),
                ))),
            },
        ]))
    }

    /// Exact reference: MaxSAT only.
    pub fn exact() -> Self {
        Self::new(Node::Atomic(AtomicAlgorithm::MaxSat(Default::default())))
    }
}
