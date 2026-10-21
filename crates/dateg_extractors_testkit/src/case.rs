use dateg_extractors::dag::ExtractError;

use super::algorithms::Algorithm;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    Fast,
    Slow,
    Soak,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Purpose {
    Verification,
    UseCase,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceKind {
    Handcrafted,
    Special,
    RandomSparse,
    RandomDense,
    Consumption,
    RandomMemory,
    UseCase,
    Failure,
}

/// Estimated size and scheduling tier of a case.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Complexity {
    pub tier: Tier,
    pub size: usize,
}

impl Complexity {
    pub const fn new(tier: Tier, size: usize) -> Self {
        Self { tier, size }
    }
}

impl Expected {
    pub fn complexity_hint(self, tier: Tier) -> Complexity {
        let size = match self {
            Self::Exact(cost) => cost.max(1),
            Self::Failure(_) | Self::Unknown => 1,
        };
        Complexity::new(tier, size)
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Override {
    Skip {
        algorithm: &'static str,
        reason: &'static str,
    },
    ExpectFailure {
        algorithm: &'static str,
        reason: &'static str,
    },
}

#[derive(Clone, Copy, Debug)]
pub enum Expected {
    Exact(usize),
    Failure(&'static str),
    /// No precomputed answer. The runner derives one from a feasible oracle
    /// when available; otherwise it runs with extraction/index validation only.
    Unknown,
}

pub type RunFn = Box<dyn Fn(&Algorithm) -> Result<usize, ExtractError> + Send + Sync + 'static>;

pub struct Case {
    pub name: &'static str,
    pub purpose: Purpose,
    pub source: SourceKind,
    pub complexity: Complexity,
    pub expected: Expected,
    pub overrides: &'static [Override],
    pub run: RunFn,
}

impl Case {
    pub fn new(
        name: &'static str,
        purpose: Purpose,
        source: SourceKind,
        complexity: Complexity,
        expected: Expected,
        run: impl Fn(&Algorithm) -> Result<usize, ExtractError> + Send + Sync + 'static,
    ) -> Self {
        Self {
            name,
            purpose,
            source,
            complexity,
            expected,
            overrides: &[],
            run: Box::new(run),
        }
    }

    pub fn with_overrides(mut self, overrides: &'static [Override]) -> Self {
        self.overrides = overrides;
        self
    }

    pub fn tier(&self) -> Tier {
        self.complexity.tier
    }
}
