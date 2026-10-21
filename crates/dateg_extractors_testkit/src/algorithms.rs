use std::{sync::LazyLock, time::Duration};

use dateg_extractors::dag::{ExtractOptions, SolverConfig};

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
        if matches!(tier, Tier::Slow | Tier::Soak) {
            algorithm.timeout = algorithm.timeout.max(Duration::from_secs(60));
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

pub static ALGORITHMS: LazyLock<Vec<Algorithm>> = LazyLock::new(|| {
    let timeout = Duration::from_secs(10);
    vec![
        Algorithm {
            name: "maxsat.inc_scc",
            config: SolverConfig::MaxSatIncSccBlocking(Default::default()),
            timeout,
            capacity: maxsat_capacity,
            oracles: &["naive"],
        },
        Algorithm {
            name: "naive",
            config: SolverConfig::Naive,
            timeout,
            capacity: naive_capacity,
            oracles: &[],
        },
    ]
});

pub fn all() -> &'static [Algorithm] {
    &ALGORITHMS
}

pub fn by_name(name: &str) -> Option<&'static Algorithm> {
    ALGORITHMS.iter().find(|algorithm| algorithm.name == name)
}
