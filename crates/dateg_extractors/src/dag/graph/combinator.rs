//! Two-stage solver: a short exact attempt, then an imprecise fallback.
//!
//! The first stage gets `first_share` of the remaining budget (capped by
//! `first_budget` when set); the second gets `second_budget` or whatever is
//! left of the overall deadline. Shares are fractions of the *remaining* time,
//! so analysis and graph construction before solving are already excluded.

use std::time::{Duration, Instant};

use super::{
    Graph, SolverConfig, max_sat_inc_scc::MaxSatIncSccExtractor, naive,
    sat_inc_scc::SatIncSccExtractor,
};
use crate::dag::{ExtractError, StatsSink};

/// Share of the remaining budget used when `first_budget` is not set.
pub const DEFAULT_FIRST_SHARE: f64 = 0.20;
/// First-stage budget when there is no overall deadline and no explicit cap.
pub const DEFAULT_FIRST_BUDGET: Duration = Duration::from_secs(5);

/// Which failures of the first stage trigger the fallback.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fallback {
    /// Only wall-clock timeouts.
    Timeout,
    /// Timeouts and "no solution found".
    TimeoutOrUnsolved,
    /// Every error.
    AnyError,
}

impl Fallback {
    fn catches(self, error: &ExtractError) -> bool {
        match error {
            ExtractError::Timeout => true,
            ExtractError::Unsolved => self != Self::Timeout,
            _ => self == Self::AnyError,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CombinatorConfig {
    pub first: Box<SolverConfig>,
    /// Fraction of the remaining budget given to the first stage.
    pub first_share: f64,
    /// Optional absolute cap on the first-stage budget.
    pub first_budget: Option<Duration>,
    pub second: Box<SolverConfig>,
    /// `None` uses the remaining overall deadline.
    pub second_budget: Option<Duration>,
    pub fallback: Fallback,
}

impl Default for CombinatorConfig {
    fn default() -> Self {
        Self {
            first: Box::new(SolverConfig::MaxSatIncScc(Default::default())),
            first_share: DEFAULT_FIRST_SHARE,
            first_budget: None,
            second: Box::new(SolverConfig::SatIncScc(Default::default())),
            second_budget: None,
            fallback: Fallback::AnyError,
        }
    }
}

#[derive(Clone, Debug)]
pub struct CombinatorConfigBuilder {
    config: CombinatorConfig,
}

impl CombinatorConfig {
    pub fn builder(first: SolverConfig) -> CombinatorConfigBuilder {
        let mut config = Self::default();
        config.first = Box::new(first);
        CombinatorConfigBuilder { config }
    }

    /// MaxSAT first, SAT fallback, with the default share and fallback policy.
    pub fn split(first: SolverConfig, second: SolverConfig) -> Self {
        Self::builder(first).second(second).build()
    }
}

impl CombinatorConfigBuilder {
    /// Fraction of the remaining budget for the first stage, in `(0, 1]`.
    pub fn first_share(mut self, share: f64) -> Self {
        assert!(
            share > 0.0 && share <= 1.0,
            "first_share must be in (0, 1], got {share}"
        );
        self.config.first_share = share;
        self
    }

    /// Absolute cap on the first-stage budget, applied after the share.
    pub fn first_budget(mut self, budget: Duration) -> Self {
        self.config.first_budget = Some(budget);
        self
    }

    pub fn second(mut self, second: SolverConfig) -> Self {
        self.config.second = Box::new(second);
        self
    }

    pub fn second_budget(mut self, budget: Duration) -> Self {
        self.config.second_budget = Some(budget);
        self
    }

    pub fn fallback(mut self, fallback: Fallback) -> Self {
        self.config.fallback = fallback;
        self
    }

    pub fn build(self) -> CombinatorConfig {
        self.config
    }
}

impl Graph {
    pub(super) fn solve_combinator(
        &mut self,
        config: &CombinatorConfig,
        deadline: Option<Instant>,
        stats: &mut StatsSink<'_>,
    ) -> Result<Vec<bool>, ExtractError> {
        let started = Instant::now();
        let first = run(self, &config.first, first_deadline(config, deadline), stats);
        stats.duration("combinator.first", started.elapsed());

        match first {
            Ok(assignment) => {
                stats.text("combinator.stage", "first");
                return Ok(assignment);
            }
            Err(error) => {
                if !config.fallback.catches(&error) {
                    return Err(error);
                }
                stats.text(
                    "combinator.first_error",
                    error.to_string().replace(' ', "_"),
                );
            }
        }

        let second_deadline = match config.second_budget {
            Some(budget) => capped(deadline, budget),
            None => deadline,
        };
        let started = Instant::now();
        let second = run(self, &config.second, second_deadline, stats);
        stats.duration("combinator.second", started.elapsed());
        second.map(|assignment| {
            stats.text("combinator.stage", "second");
            assignment
        })
    }
}

fn first_deadline(config: &CombinatorConfig, deadline: Option<Instant>) -> Option<Instant> {
    let budget = match deadline {
        Some(overall) => {
            let remaining = overall.saturating_duration_since(Instant::now());
            let share =
                Duration::from_secs_f64((remaining.as_secs_f64() * config.first_share).max(0.0));
            config.first_budget.map_or(share, |cap| share.min(cap))
        }
        None => config.first_budget.unwrap_or(DEFAULT_FIRST_BUDGET),
    };
    capped(deadline, budget)
}

fn capped(deadline: Option<Instant>, budget: Duration) -> Option<Instant> {
    let own = Instant::now() + budget;
    Some(deadline.map_or(own, |overall| overall.min(own)))
}

fn run(
    graph: &mut Graph,
    config: &SolverConfig,
    deadline: Option<Instant>,
    stats: &mut StatsSink<'_>,
) -> Result<Vec<bool>, ExtractError> {
    match config {
        SolverConfig::Naive => naive::solve(graph, deadline).map(|(_, assignment)| assignment),
        SolverConfig::MaxSatIncScc(config) => {
            MaxSatIncSccExtractor::new(graph, config, deadline)?.solve(deadline, stats)
        }
        SolverConfig::SatIncScc(config) => {
            SatIncSccExtractor::new(graph, config, deadline)?.solve(deadline, stats)
        }
        SolverConfig::Combinator(_) => Err(ExtractError::Other(anyhow::anyhow!(
            "nested combinator configs are not supported"
        ))),
    }
}
