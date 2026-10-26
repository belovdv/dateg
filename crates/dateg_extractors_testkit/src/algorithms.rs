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

fn labelled(name: &'static str, node: Node) -> Node {
    Node::Label {
        name,
        child: Box::new(node),
    }
}

fn timeout(policy: TimeoutPolicy, node: Node) -> Node {
    Node::Timeout {
        policy,
        child: Box::new(node),
    }
}

/// Temporary reconstruction of the old two-stage combinator.
///
/// `final_result` picks the old final behavior (stop after a successful first
/// stage) or fallback behavior (always run the second stage).
fn combinator_tree(
    first: AtomicAlgorithm,
    first_policy: TimeoutPolicy,
    second: AtomicAlgorithm,
    second_policy: TimeoutPolicy,
    final_result: bool,
) -> SolverConfig {
    let first = labelled("first", timeout(first_policy, Node::Atomic(first)));
    let second = labelled("second", timeout(second_policy, Node::Atomic(second)));
    let baseline = labelled(
        "baseline",
        Node::Atomic(AtomicAlgorithm::Chain(BestEffortConfig::default())),
    );
    let pipeline = if final_result {
        Node::Try {
            primary: Box::new(first),
            fallback: Box::new(Node::Bundle(vec![baseline, second])),
        }
    } else {
        Node::Bundle(vec![
            Node::OnFailure {
                primary: Box::new(first),
                fallback: Box::new(baseline),
            },
            second,
        ])
    };
    let mut config = SolverConfig::new(pipeline);
    config.name = Some("combinator");
    config
}

/// Temporary combinator helper with explicit budgets; used by `probe`/tests.
pub fn combinator_with_budgets(
    first_share: f64,
    first_budget: Option<Duration>,
    second_budget: Option<Duration>,
) -> SolverConfig {
    let first_policy = match first_budget {
        Some(cap) => TimeoutPolicy::FractionCapped {
            fraction: first_share,
            cap,
        },
        None => TimeoutPolicy::Fraction(first_share),
    };
    let second_policy = match second_budget {
        Some(cap) => TimeoutPolicy::Time(cap),
        None => TimeoutPolicy::Remaining,
    };
    combinator_tree(
        AtomicAlgorithm::MaxSat(Default::default()),
        first_policy,
        AtomicAlgorithm::SatDescending(Default::default()),
        second_policy,
        true,
    )
}

/// Rebuild a legacy combinator entry with probe overrides; `None` for a
/// non-combinator algorithm.
pub fn with_legacy_budgets(
    name: &str,
    first_share: Option<f64>,
    first_budget: Option<Duration>,
    second_budget: Option<Duration>,
) -> Option<SolverConfig> {
    if !name.starts_with("combinator") {
        return None;
    }
    if name == "combinator.chain" {
        let first_policy = match first_budget.or(Some(Duration::from_secs(2))) {
            Some(cap) => TimeoutPolicy::FractionCapped {
                fraction: first_share.unwrap_or(0.05),
                cap,
            },
            None => TimeoutPolicy::Fraction(first_share.unwrap_or(0.05)),
        };
        let second_policy = match second_budget {
            Some(cap) => TimeoutPolicy::Time(cap),
            None => TimeoutPolicy::Remaining,
        };
        return Some(combinator_tree(
            AtomicAlgorithm::Chain(BestEffortConfig::default()),
            first_policy,
            AtomicAlgorithm::MaxSat(Default::default()),
            second_policy,
            false,
        ));
    }
    Some(combinator_with_budgets(
        first_share.unwrap_or(0.20),
        first_budget,
        second_budget,
    ))
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
            name: "chain",
            config: chain(),
            filtration: None,
            capacity: sat_capacity,
            oracles: &[],
            bench_only: true,
        },
        Algorithm {
            name: "combinator.chain",
            config: combinator_tree(
                AtomicAlgorithm::Chain(BestEffortConfig::default()),
                TimeoutPolicy::FractionCapped {
                    fraction: 0.05,
                    cap: Duration::from_secs(2),
                },
                AtomicAlgorithm::MaxSat(Default::default()),
                TimeoutPolicy::Remaining,
                false,
            ),
            filtration: Some(FilterMode::AtLeastOneSolution),
            capacity: sat_capacity,
            oracles: &[],
            bench_only: true,
        },
        Algorithm {
            name: "combinator",
            config: combinator_with_budgets(0.20, None, None),
            filtration: Some(FilterMode::AtLeastOneSolution),
            capacity: sat_capacity,
            oracles: &[],
            bench_only: true,
        },
        Algorithm {
            name: "combinator.optimal",
            config: combinator_with_budgets(0.20, None, None),
            filtration: Some(FilterMode::AtLeastOneOptimal),
            capacity: sat_capacity,
            oracles: &[],
            bench_only: true,
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
        assert_eq!(matching("sat.*").len(), 2);
        assert!(matching("sat.*").iter().all(|a| a.name.starts_with("sat.")));
        assert_eq!(matching("combinator*").len(), 4);
        assert_eq!(matching("chain").len(), 1);
        assert!(matching("no.such.algorithm").is_empty());
    }
}
