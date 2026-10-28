use std::{sync::LazyLock, time::Duration};

use dateg_extractors::dag::{
    AtomicAlgorithm, BestEffortConfig, ExtractOptions, FilterMode, Node, SatDescendingConfig,
    SolverConfig, TimeoutPolicy,
};

use crate::case::{Complexity, Tier};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fit {
    Fast,
    Slow,
    Infeasible,
}

#[derive(Clone, Debug)]
pub struct Algorithm {
    pub name: &'static str,
    pub config: SolverConfig,
    /// Optional graph reduction applied before solving.
    pub filtration: Option<FilterMode>,
    /// Maps case complexity to feasibility; calibrate from benchmarks/timeout
    /// data. `Slow` still runs, `Infeasible` is skipped.
    pub capacity: fn(Complexity) -> Fit,
    /// Simpler implementations used as an oracle when a case has no
    /// precomputed answer; ordered from simplest to most expensive.
    pub oracles: &'static [&'static str],
    /// Excluded from the verification matrix, but available to the bench.
    /// Used for solvers whose result depends on the timeout (imprecise).
    pub bench_only: bool,
}

impl Algorithm {
    pub fn options(&self) -> ExtractOptions {
        ExtractOptions::builder(self.config.clone())
            .filtration(self.filtration)
            .build()
    }

    pub fn fit(&self, complexity: Complexity) -> Fit {
        (self.capacity)(complexity)
    }

    /// Slow/soak cases may use a larger solver budget; fast cases stay short.
    pub fn for_tier(&self, tier: Tier) -> Algorithm {
        let mut algorithm = self.clone();
        let floor = match tier {
            Tier::Fast => return algorithm,
            Tier::Slow => Duration::from_secs(60),
            Tier::Soak => Duration::from_secs(600),
        };
        if let Some(timeout) = algorithm.config.timeout {
            algorithm.config.timeout = Some(timeout.max(floor));
        }
        algorithm
    }
}

fn atomic(algorithm: AtomicAlgorithm) -> SolverConfig {
    SolverConfig::new(Node::Atomic(algorithm))
}

fn maxsat() -> SolverConfig {
    atomic(AtomicAlgorithm::MaxSat(Default::default()))
}

fn sat() -> SolverConfig {
    atomic(AtomicAlgorithm::SatDescending(
        SatDescendingConfig::default(),
    ))
}

fn chain() -> SolverConfig {
    atomic(AtomicAlgorithm::Chain(BestEffortConfig::default()))
}

fn component_filter() -> Node {
    Node::Timeout {
        policy: TimeoutPolicy::FractionCapped {
            fraction: 0.5,
            cap: Duration::from_secs(5),
        },
        child: Box::new(Node::Filter(FilterMode::ComponentOptimal)),
    }
}

fn maxsat_component() -> SolverConfig {
    SolverConfig::new(Node::Bundle(vec![
        Node::Label {
            name: "first",
            child: Box::new(Node::Atomic(AtomicAlgorithm::Chain(
                BestEffortConfig::default(),
            ))),
        },
        component_filter(),
        Node::Label {
            name: "second",
            child: Box::new(Node::Atomic(AtomicAlgorithm::MaxSat(Default::default()))),
        },
    ]))
}

/// Measured on a chain: complexity 14 ~0.4 s, 16 ~6 s, 17 hits the 10 s
/// timeout. This is shape-dependent; refine with timeout data.
fn naive_capacity(complexity: Complexity) -> Fit {
    let (slow, max) = match complexity.tier {
        Tier::Fast => (11, 14),
        Tier::Slow | Tier::Soak => (13, 16),
    };
    if complexity.size > max {
        Fit::Infeasible
    } else if complexity.size > slow {
        Fit::Slow
    } else {
        Fit::Fast
    }
}

/// Maxsat capacity is not measured yet, so it never filters.
fn maxsat_capacity(_: Complexity) -> Fit {
    Fit::Fast
}

/// Not calibrated; bench-only entries are filtered out of the matrix.
fn sat_capacity(_: Complexity) -> Fit {
    Fit::Fast
}

pub static ALGORITHMS: LazyLock<Vec<Algorithm>> = LazyLock::new(|| {
    vec![
        Algorithm {
            name: "maxsat.inc_scc",
            config: maxsat(),
            filtration: None,
            capacity: maxsat_capacity,
            oracles: &["naive"],
            bench_only: false,
        },
        Algorithm {
            name: "maxsat.filtered",
            config: maxsat(),
            filtration: Some(FilterMode::AtLeastOneOptimal),
            capacity: maxsat_capacity,
            oracles: &["naive"],
            bench_only: false,
        },
        Algorithm {
            name: "maxsat.dead",
            config: maxsat(),
            filtration: Some(FilterMode::DeadVertices),
            capacity: maxsat_capacity,
            oracles: &["naive"],
            bench_only: false,
        },
        Algorithm {
            name: "sat.descending",
            config: sat(),
            filtration: None,
            capacity: sat_capacity,
            oracles: &[],
            bench_only: true,
        },
        Algorithm {
            name: "sat.descending.filtered",
            config: sat(),
            filtration: Some(FilterMode::AtLeastOneSolution),
            capacity: sat_capacity,
            oracles: &[],
            bench_only: true,
        },
        Algorithm {
            name: "sat.descending.dead",
            config: sat(),
            filtration: Some(FilterMode::DeadVertices),
            capacity: sat_capacity,
            oracles: &[],
            bench_only: true,
        },
        Algorithm {
            name: "sat.descending.bottom_up",
            config: sat(),
            filtration: Some(FilterMode::ComponentBottomUp),
            capacity: sat_capacity,
            oracles: &[],
            bench_only: true,
        },
        Algorithm {
            name: "sat.descending.frontier",
            config: sat(),
            filtration: Some(FilterMode::ComponentBottomUpFrontier(16)),
            capacity: sat_capacity,
            oracles: &[],
            bench_only: true,
        },
        Algorithm {
            name: "sat.descending.frontier64",
            config: sat(),
            filtration: Some(FilterMode::ComponentBottomUpFrontier(64)),
            capacity: sat_capacity,
            oracles: &[],
            bench_only: true,
        },
        Algorithm {
            name: "chain",
            config: chain(),
            filtration: None,
            capacity: sat_capacity,
            oracles: &[],
            bench_only: true,
        },
        Algorithm {
            name: "combinator.component",
            config: maxsat_component(),
            filtration: None,
            capacity: sat_capacity,
            oracles: &["maxsat.inc_scc"],
            bench_only: false,
        },
        Algorithm {
            name: "combinator.thorough",
            config: SolverConfig::thorough(),
            filtration: None,
            capacity: sat_capacity,
            oracles: &[],
            bench_only: true,
        },
        Algorithm {
            name: "naive",
            config: atomic(AtomicAlgorithm::Naive),
            filtration: None,
            capacity: naive_capacity,
            oracles: &[],
            bench_only: false,
        },
    ]
});

pub fn all() -> &'static [Algorithm] {
    &ALGORITHMS
}

pub fn by_name(name: &str) -> Option<&'static Algorithm> {
    ALGORITHMS.iter().find(|algorithm| algorithm.name == name)
}

/// Select algorithms by name. `*` and `?` are wildcards; without a wildcard
/// the pattern must match the name exactly.
pub fn matching(pattern: &str) -> Vec<&'static Algorithm> {
    ALGORITHMS
        .iter()
        .filter(|algorithm| matches_pattern(pattern, algorithm.name))
        .collect()
}

fn matches_pattern(pattern: &str, name: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let name: Vec<char> = name.chars().collect();
    let (mut pi, mut ni) = (0, 0);
    let mut star = None;
    let mut star_next = 0;

    while ni < name.len() {
        if pi < pattern.len() && (pattern[pi] == '?' || pattern[pi] == name[ni]) {
            pi += 1;
            ni += 1;
        } else if pi < pattern.len() && pattern[pi] == '*' {
            star = Some(pi);
            star_next = ni;
            pi += 1;
        } else if let Some(star) = star {
            pi = star + 1;
            star_next += 1;
            ni = star_next;
        } else {
            return false;
        }
    }
    while pi < pattern.len() && pattern[pi] == '*' {
        pi += 1;
    }
    pi == pattern.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_algorithm_names() {
        assert_eq!(matching("maxsat.inc_scc").len(), 1);
        assert_eq!(matching("sat.*").len(), 6);
        assert!(matching("sat.*").iter().all(|a| a.name.starts_with("sat.")));
        assert_eq!(matching("combinator*").len(), 2);
        assert_eq!(matching("chain").len(), 1);
        assert!(matching("no.such.algorithm").is_empty());
    }
}
