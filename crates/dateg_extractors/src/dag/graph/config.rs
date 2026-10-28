use std::time::Duration;

use super::{AtomicAlgorithm, FilterMode, Node, SolverConfig, TimeoutPolicy};

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

    /// Minimal: dead-vertex cleanup, then descending SAT only.
    pub fn fast() -> Self {
        Self::new(Node::Try {
            primary: Box::new(dead_vertices()),
            fallback: Box::new(Node::Atomic(AtomicAlgorithm::SatDescending(
                Default::default(),
            ))),
        })
    }

    /// Dead-vertex cleanup, chain UB, then descending SAT.
    pub fn balanced() -> Self {
        Self::new(Node::Try {
            primary: Box::new(dead_vertices()),
            fallback: Box::new(Node::Bundle(vec![
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
            ])),
        })
    }

    /// Reliability-oriented: dead-vertex cleanup, then chain, structural filter,
    /// exact MaxSAT, and SAT with 80% of the budget. On failure, spend the
    /// remaining 10% on the validity-preserving support-frontier fallback.
    pub fn thorough() -> Self {
        Self::new(Node::Try {
            primary: Box::new(dead_vertices()),
            fallback: Box::new(Node::OnFailure {
                primary: Box::new(Node::Timeout {
                    policy: TimeoutPolicy::Fraction(0.9),
                    child: Box::new(Node::Bundle(vec![
                        Node::Label {
                            name: "first",
                            child: Box::new(Node::Atomic(AtomicAlgorithm::Chain(
                                Default::default(),
                            ))),
                        },
                        Node::Filter(FilterMode::AtLeastOneOptimal),
                        Node::Label {
                            name: "baseline",
                            child: Box::new(Node::Atomic(AtomicAlgorithm::MaxSat(
                                Default::default(),
                            ))),
                        },
                        Node::Label {
                            name: "second",
                            child: Box::new(Node::Atomic(AtomicAlgorithm::SatDescending(
                                Default::default(),
                            ))),
                        },
                    ])),
                }),
                fallback: Box::new(Node::Bundle(vec![
                    Node::Label {
                        name: "bottom_up",
                        child: Box::new(Node::Filter(FilterMode::ComponentBottomUpFrontier(16))),
                    },
                    Node::Label {
                        name: "hammer",
                        child: Box::new(Node::Atomic(AtomicAlgorithm::SatDescending(
                            Default::default(),
                        ))),
                    },
                ])),
            }),
        })
    }

    /// Exact reference: dead-vertex cleanup, then MaxSAT only.
    pub fn exact() -> Self {
        Self::new(Node::Try {
            primary: Box::new(dead_vertices()),
            fallback: Box::new(Node::Atomic(AtomicAlgorithm::MaxSat(Default::default()))),
        })
    }
}

fn dead_vertices() -> Node {
    Node::Filter(FilterMode::DeadVertices)
}
