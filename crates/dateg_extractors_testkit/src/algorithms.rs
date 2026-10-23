use std::{sync::LazyLock, time::Duration};

use dateg_extractors::dag::{ExtractOptions, SatIncSccConfig, SolverConfig};

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
    pub timeout: Duration,
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
            .timeout(Some(self.timeout))
            .build()
    }

    pub fn fit(&self, complexity: Complexity) -> Fit {
        (self.capacity)(complexity)
    }

    /// Slow/soak cases may use a larger solver budget; fast cases stay short.
    pub fn for_tier(&self, tier: Tier) -> Algorithm {
        let mut algorithm = self.clone();
        match tier {
            Tier::Fast => {}
            Tier::Slow => algorithm.timeout = algorithm.timeout.max(Duration::from_secs(60)),
            Tier::Soak => algorithm.timeout = algorithm.timeout.max(Duration::from_secs(600)),
        }
        algorithm
    }
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
    let timeout = Duration::from_secs(10);
    vec![
        Algorithm {
            name: "maxsat.inc_scc",
            config: SolverConfig::MaxSatIncScc(Default::default()),
            timeout,
            capacity: maxsat_capacity,
            oracles: &["naive"],
            bench_only: false,
        },
        Algorithm {
            name: "sat.inc_scc",
            config: SolverConfig::SatIncScc(SatIncSccConfig::default()),
            timeout,
            capacity: sat_capacity,
            oracles: &[],
            bench_only: true,
        },
        Algorithm {
            name: "naive",
            config: SolverConfig::Naive,
            timeout,
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
        assert_eq!(matching("sat.*").len(), 1);
        assert!(matching("sat.*").iter().all(|a| a.name == "sat.inc_scc"));
        assert!(matching("no.such.algorithm").is_empty());
    }
}
