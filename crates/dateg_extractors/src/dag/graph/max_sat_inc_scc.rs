use std::time::Instant;

use super::common::{Check, SmtCore};
use super::{CycleBlocking, Graph, MaxSatIncSccConfig, VertexId, View};
use crate::dag::{Error, StatsSink, check_deadline};

pub(super) struct MaxSatIncSccExtractor<'a> {
    core: SmtCore<'a>,
    preemptive_4cycles: bool,
    cycle_blocking: CycleBlocking,
    soft: Option<Instant>,
}

enum Step {
    Solved(Vec<bool>),
    Blocked { cycles: usize },
}

impl<'a> MaxSatIncSccExtractor<'a> {
    pub(super) fn new(
        g: &'a Graph,
        roots: &'a [VertexId],
        view: &'a View,
        config: &MaxSatIncSccConfig,
        deadline: Option<Instant>,
        soft: Option<Instant>,
    ) -> Result<Self, Error> {
        let core = SmtCore::new(g, roots, view, deadline)?;
        let mut r = Self {
            core,
            preemptive_4cycles: config.preemptive_4cycles,
            cycle_blocking: config.cycle_blocking,
            soft,
        };

        for node in r.core.g.graph.node_indices() {
            if !r.core.domain[node.index()] {
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
    fn try_solve(&mut self) -> Result<Step, Error> {
        match self.core.check(None)? {
            Check::Unsat => Err(Error::Infeasible),
            Check::Sat(selected) => {
                let blocked = self.core.block_cycles(&selected, self.cycle_blocking);
                if blocked.is_empty() {
                    Ok(Step::Solved(selected))
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
    ) -> Result<Vec<bool>, Error> {
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
            if self.soft.is_some_and(|soft| Instant::now() >= soft) {
                return Err(Error::Timeout);
            }
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
