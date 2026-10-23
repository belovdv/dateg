use std::time::Instant;

use super::Graph;
use super::common::{Check, CycleBlocking, SmtCore};
use crate::dag::{ExtractError, StatsSink, check_deadline};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MaxSatIncSccConfig {
    /// Preemptively block all 4-cycles before the first solve.
    pub preemptive_4cycles: bool,
    /// How cyclic models are refined.
    pub cycle_blocking: CycleBlocking,
}

impl Default for MaxSatIncSccConfig {
    fn default() -> Self {
        Self {
            preemptive_4cycles: true,
            cycle_blocking: CycleBlocking::default(),
        }
    }
}

pub(super) struct MaxSatIncSccExtractor<'a> {
    core: SmtCore<'a>,
    preemptive_4cycles: bool,
    cycle_blocking: CycleBlocking,
}

enum Step {
    Solved(Vec<bool>),
    Blocked { cycles: usize },
}

impl<'a> MaxSatIncSccExtractor<'a> {
    pub(super) fn new(
        g: &'a Graph,
        config: &MaxSatIncSccConfig,
        deadline: Option<Instant>,
    ) -> Result<Self, ExtractError> {
        let core = SmtCore::new(g, deadline)?;
        let mut r = Self {
            core,
            preemptive_4cycles: config.preemptive_4cycles,
            cycle_blocking: config.cycle_blocking,
        };

        for node in r.core.g.graph.node_indices() {
            if !r.core.g.used[node.index()] {
                continue;
            }
            let cost = r.core.g.graph[node].cost;
            if cost == 0 {
                continue;
            }
            let assert_soft = r.core.ctx.list(vec![
                r.core.ctx.atom("assert-soft"),
                r.core.ctx.not(r.core.vars[node.index()]),
                r.core.ctx.atom(":weight"),
                r.core.ctx.numeral(cost),
            ]);
            r.core.ctx.raw_send(assert_soft).unwrap();
            r.core.ctx.raw_recv().unwrap();
        }
        r.core.assert_conflict_groups();

        Ok(r)
    }

    /// Increment for search.
    fn try_solve(&mut self) -> Result<Step, ExtractError> {
        match self.core.check(None)? {
            Check::Unsat => Err(ExtractError::Unsolved),
            Check::Sat(used) => {
                let blocked = self.core.block_cycles(&used, self.cycle_blocking);
                if blocked.is_empty() {
                    Ok(Step::Solved(used))
                } else {
                    Ok(Step::Blocked {
                        cycles: blocked.len(),
                    })
                }
            }
        }
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
            stats.count("maxsat.inc_scc.preemptive_4cycles", cycles as u64);
        }
        stats.duration("maxsat.inc_scc.preemptive", preemptive_started.elapsed());
        stats.text(
            "maxsat.inc_scc.cycle_blocking",
            self.cycle_blocking.as_str(),
        );

        let mut steps = 0u64;
        let mut blocked_sccs = 0u64;
        loop {
            check_deadline(deadline)?;
            steps += 1;
            let step = self.try_solve();
            if let Ok(Step::Blocked { cycles }) = &step {
                blocked_sccs += *cycles as u64;
            }
            stats.count("maxsat.inc_scc.steps", steps);
            stats.count("maxsat.inc_scc.blocked_sccs", blocked_sccs);
            match step? {
                Step::Solved(solution) => return Ok(solution),
                Step::Blocked { .. } => {}
            }
        }
    }
}
