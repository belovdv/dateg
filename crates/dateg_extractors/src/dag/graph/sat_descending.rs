use std::time::Instant;

use easy_smt::SExpr;

use super::common::{Check, SmtCore};
use super::{
    CycleBlocking, Graph, PreemptiveCyclesConfig, SatDescendingConfig, VertexId, View,
    assignment_cost, check_constraints,
};
use crate::dag::{Error, StatsSink, check_deadline};

/// Plain SAT plus a cost-bound assertion, refined by repeated probes.
///
/// The first model has no cost bound, so it only has to be acyclic. Its cost,
/// or an optionally supplied validated assignment, is the initial upper bound;
/// probes between `0` and that bound tighten it. Each probe activates a fresh
/// assumption literal, so earlier assertions and learned clauses remain
/// available. If the deadline expires, the best acyclic model found so far (or
/// the supplied hint assignment) is kept.
pub(super) struct SatDescendingResult {
    pub assignment: Vec<bool>,
    pub lower_bound: Option<usize>,
    pub optimal: bool,
}

pub(super) struct SatDescendingExtractor<'a> {
    core: SmtCore<'a>,
    total_cost: SExpr,
    preemptive: Option<PreemptiveCyclesConfig>,
    cycle_blocking: CycleBlocking,
    descent_fraction: f64,
    probe_checks: u64,
    probe_blocked_sets: u64,
    /// Validated known solution used as the initial upper bound.
    hint: Option<(usize, Vec<bool>)>,
    /// Stop improving once a solution exists and this deadline is reached.
    soft: Option<Instant>,
}

enum Probe {
    Feasible(Vec<bool>),
    Infeasible,
    TimedOut,
}

impl<'a> SatDescendingExtractor<'a> {
    pub(super) fn new(
        g: &'a Graph,
        roots: &'a [VertexId],
        view: &'a View,
        config: &SatDescendingConfig,
        deadline: Option<Instant>,
        soft: Option<Instant>,
        hint: Option<Vec<bool>>,
    ) -> Result<Self, Error> {
        let hint = hint
            .filter(|assignment| check_constraints(g, roots, view, assignment))
            .map(|assignment| (assignment_cost(g, view, &assignment), assignment));
        let mut core = SmtCore::new(g, roots, view, deadline)?;
        core.assert_root_closure();
        core.assert_conflict_groups();
        let total_cost = core.assert_total_cost();
        Ok(Self {
            core,
            total_cost,
            preemptive: config.preemptive_cycles.clone(),
            cycle_blocking: config.cycle_blocking,
            descent_fraction: config.descent_fraction,
            probe_checks: 0,
            probe_blocked_sets: 0,
            hint,
            soft,
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
    fn probe(&mut self, bound: usize) -> Result<Probe, Error> {
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
                Err(Error::Timeout) => return Ok(Probe::TimedOut),
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
    ) -> Result<SatDescendingResult, Error> {
        check_deadline(deadline)?;
        let preemptive_started = Instant::now();
        if let Some(config) = &self.preemptive {
            let report = self.core.assert_preemptive_cycles(config);
            stats.count("sat.descending.preemptive_cycles", report.cycles as u64);
            stats.count("sat.descending.preemptive_literals", report.literals as u64);
            stats.count("sat.descending.preemptive_visited", report.visited as u64);
            stats.count(
                "sat.descending.preemptive_truncated",
                u64::from(report.truncated),
            );
            stats.count("sat.descending.preemptive_length", report.reached as u64);
            stats.count("sat.descending.preemptive_ceiling", report.ceiling as u64);
        }
        stats.duration("sat.descending.preemptive", preemptive_started.elapsed());

        let mut initial_steps = 0u64;
        let mut initial_blocked_sets = 0u64;
        let initial_started = Instant::now();
        let hint = self.hint.take();
        let has_hint = hint.is_some();
        stats.count("sat.descending.has_hint", u64::from(has_hint));
        if let Some((cost, _)) = &hint {
            stats.count("sat.descending.hint_cost", *cost as u64);
        }
        let first = 'first: loop {
            if check_deadline(deadline).is_err() {
                break 'first Err(Error::Timeout);
            }
            initial_steps += 1;
            stats.count("sat.descending.initial_steps", initial_steps);
            match self.core.check(None) {
                Ok(Check::Unsat) => break 'first Err(Error::Infeasible),
                Ok(Check::Sat(selected)) => {
                    let blocked = self.core.block_cycles(&selected, self.cycle_blocking);
                    if blocked.is_empty() {
                        let cost = self.core.cost(&selected);
                        break 'first Ok((cost, selected));
                    }
                    initial_blocked_sets += blocked.len() as u64;
                }
                Err(Error::Timeout) => break 'first Err(Error::Timeout),
                Err(error) => break 'first Err(error),
            }
        };
        let mut timed_out = self.soft.is_some_and(|soft| Instant::now() >= soft);
        let (mut best_cost, mut best) = match first {
            Ok(first) => match hint {
                Some(hint) if hint.0 <= first.0 => hint,
                _ => first,
            },
            Err(error @ (Error::Timeout | Error::NotFound | Error::Infeasible)) => match hint {
                Some(hint) => {
                    timed_out = true;
                    hint
                }
                None => return Err(error),
            },
            Err(error) => return Err(error),
        };
        stats.count("sat.descending.initial_blocked_sccs", initial_blocked_sets);
        stats.count("sat.descending.initial_cost", best_cost as u64);
        stats.duration("sat.descending.initial", initial_started.elapsed());

        let mut lower = None;
        let mut probes = 0u64;
        while !timed_out && let Some(bound) = self.next_bound(lower, best_cost) {
            if self.soft.is_some_and(|soft| Instant::now() >= soft) {
                timed_out = true;
                break;
            }
            if check_deadline(deadline).is_err() {
                timed_out = true;
                break;
            }
            probes += 1;
            match self.probe(bound)? {
                Probe::Infeasible => lower = Some(bound),
                Probe::Feasible(selected) => {
                    let cost = self.core.cost(&selected);
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

        stats.count("sat.descending.probes", probes);
        stats.count("sat.descending.probe_steps", self.probe_checks);
        stats.count("sat.descending.blocked_sccs", self.probe_blocked_sets);
        stats.text(
            "sat.descending.cycle_blocking",
            self.cycle_blocking.as_str(),
        );
        stats.text(
            "sat.descending.descent_fraction",
            format!("{:.6}", self.descent_fraction),
        );
        stats.count("sat.descending.final_cost", best_cost as u64);
        stats.count(
            "sat.descending.lower_bound",
            lower.map(|lower| (lower + 1) as u64).unwrap_or(0),
        );
        stats.count("sat.descending.timed_out", u64::from(timed_out));
        stats.count("sat.descending.optimal", u64::from(!timed_out));

        Ok(SatDescendingResult {
            assignment: best,
            lower_bound: if timed_out {
                lower.map(|lower| lower + 1)
            } else {
                Some(best_cost)
            },
            optimal: !timed_out,
        })
    }
}
