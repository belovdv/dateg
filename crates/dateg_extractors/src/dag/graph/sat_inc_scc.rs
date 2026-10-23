use std::time::Instant;

use easy_smt::SExpr;

use super::Graph;
use super::common::{Check, CycleBlocking, SmtCore};
use crate::dag::{ExtractError, StatsSink, check_deadline};

const DEFAULT_DESCENT_FRACTION: f64 = 1.0 / 64.0;

/// Plain SAT plus a cost-bound assertion, refined by repeated probes.
///
/// The first model has no cost bound, so it only has to be acyclic. Its cost is
/// an upper bound for the search; probes between `0` and that bound tighten it.
/// Each probe activates a fresh assumption literal, so earlier assertions and
/// learned clauses remain available. If the deadline expires during
/// improvement, the best acyclic model found so far is kept.
pub(super) struct SatIncSccExtractor<'a> {
    core: SmtCore<'a>,
    total_cost: SExpr,
    preemptive_4cycles: bool,
    cycle_blocking: CycleBlocking,
    descent_fraction: f64,
    probe_checks: u64,
    probe_blocked_sets: u64,
}

enum Probe {
    Feasible(Vec<bool>),
    Infeasible,
    TimedOut,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SatIncSccConfig {
    /// Preemptively block all 4-cycles before the first solve.
    pub preemptive_4cycles: bool,
    /// How cyclic models are refined.
    pub cycle_blocking: CycleBlocking,
    /// Fraction of the remaining gap to descend by on each probe.
    pub descent_fraction: f64,
}

impl Default for SatIncSccConfig {
    fn default() -> Self {
        Self {
            preemptive_4cycles: true,
            cycle_blocking: CycleBlocking::default(),
            descent_fraction: DEFAULT_DESCENT_FRACTION,
        }
    }
}

impl<'a> SatIncSccExtractor<'a> {
    pub(super) fn new(
        g: &'a Graph,
        config: &SatIncSccConfig,
        deadline: Option<Instant>,
    ) -> Result<Self, ExtractError> {
        let mut core = SmtCore::new(g, deadline)?;
        core.assert_root_closure();
        core.assert_conflict_groups();
        let total_cost = core.assert_total_cost();
        Ok(Self {
            core,
            total_cost,
            preemptive_4cycles: config.preemptive_4cycles,
            cycle_blocking: config.cycle_blocking,
            descent_fraction: config.descent_fraction,
            probe_checks: 0,
            probe_blocked_sets: 0,
        })
    }

    /// Add an assumption literal that implies `total_cost <= bound`.
    fn assume_bound(&mut self, bound: usize) -> SExpr {
        let assumption = self.core.new_var("_bound", self.core.ctx.bool_sort());
        let bound = self
            .core
            .ctx
            .lte(self.total_cost, self.core.ctx.numeral(bound));
        self.core
            .ctx
            .assert(self.core.ctx.imp(assumption, bound))
            .unwrap();
        assumption
    }

    /// Check one cost bound, refining cycles until the model is acyclic.
    fn probe(&mut self, bound: usize) -> Result<Probe, ExtractError> {
        let assumption = self.assume_bound(bound);
        loop {
            self.probe_checks += 1;
            match self.core.check(Some(assumption.clone())) {
                Ok(Check::Unsat) => return Ok(Probe::Infeasible),
                Ok(Check::Sat(selected)) => {
                    let blocked = self.core.block_cycles(&selected, self.cycle_blocking);
                    if blocked.is_empty() {
                        return Ok(Probe::Feasible(selected));
                    }
                    self.probe_blocked_sets += blocked.len() as u64;
                }
                Err(ExtractError::Timeout) => return Ok(Probe::TimedOut),
                Err(error) => return Err(error),
            }
        }
    }

    /// Next cost bound to probe, or `None` when the interval proves optimality.
    fn next_bound(&self, lower: Option<usize>, best: usize) -> Option<usize> {
        if best == 0 || lower.is_some_and(|lower| best <= lower + 1) {
            return None;
        }
        let gap = best - lower.unwrap_or(0);
        let step = ((gap as f64 * self.descent_fraction).floor() as usize).max(1);
        Some(best - step)
    }

    pub(super) fn solve(
        &mut self,
        deadline: Option<Instant>,
        stats: &mut StatsSink<'_>,
    ) -> Result<Vec<bool>, ExtractError> {
        check_deadline(deadline)?;
        let preemptive_started = Instant::now();
        if self.preemptive_4cycles {
            let cycles = self.core.block_4cycles();
            stats.count("sat.inc_scc.preemptive_4cycles", cycles as u64);
        }
        stats.duration("sat.inc_scc.preemptive", preemptive_started.elapsed());

        let mut initial_steps = 0u64;
        let mut initial_blocked_sets = 0u64;
        let (mut best_cost, mut best) = loop {
            check_deadline(deadline)?;
            initial_steps += 1;
            stats.count("sat.inc_scc.initial_steps", initial_steps);
            match self.core.check(None)? {
                Check::Unsat => return Err(ExtractError::Unsolved),
                Check::Sat(selected) => {
                    let blocked = self.core.block_cycles(&selected, self.cycle_blocking);
                    if blocked.is_empty() {
                        let cost = self.core.g.assignment_cost(&selected);
                        break (cost, selected);
                    }
                    initial_blocked_sets += blocked.len() as u64;
                }
            }
        };
        stats.count("sat.inc_scc.initial_blocked_sccs", initial_blocked_sets);
        stats.count("sat.inc_scc.initial_cost", best_cost as u64);

        let mut lower = None;
        let mut probes = 0u64;
        let mut timed_out = false;
        while let Some(bound) = self.next_bound(lower, best_cost) {
            if check_deadline(deadline).is_err() {
                timed_out = true;
                break;
            }
            probes += 1;
            match self.probe(bound)? {
                Probe::Infeasible => lower = Some(bound),
                Probe::Feasible(selected) => {
                    let cost = self.core.g.assignment_cost(&selected);
                    if cost < best_cost {
                        best_cost = cost;
                        best = selected;
                    }
                }
                Probe::TimedOut => {
                    timed_out = true;
                    break;
                }
            }
        }

        stats.count("sat.inc_scc.probes", probes);
        stats.count("sat.inc_scc.probe_steps", self.probe_checks);
        stats.count("sat.inc_scc.blocked_sccs", self.probe_blocked_sets);
        stats.text("sat.inc_scc.cycle_blocking", self.cycle_blocking.as_str());
        stats.text(
            "sat.inc_scc.descent_fraction",
            format!("{:.6}", self.descent_fraction),
        );
        stats.count("sat.inc_scc.final_cost", best_cost as u64);
        stats.count(
            "sat.inc_scc.lower_bound",
            lower.map(|lower| (lower + 1) as u64).unwrap_or(0),
        );
        stats.count("sat.inc_scc.timed_out", u64::from(timed_out));
        stats.count("sat.inc_scc.optimal", u64::from(!timed_out));

        Ok(best)
    }
}
