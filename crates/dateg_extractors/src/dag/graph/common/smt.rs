use std::time::Instant;

use easy_smt::{Context, SExpr};
use petgraph::{
    data::DataMap,
    visit::{IntoNeighbors, IntoNeighborsDirected, IntoNodeIdentifiers, NodeFiltered},
};

use super::super::{Graph, VertexId};
use crate::dag::{ExtractError, check_deadline, remaining};

pub(crate) enum Check {
    Sat(Vec<bool>),
    Unsat,
}

/// How a model with cycles is refined.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CycleBlocking {
    /// Block every vertex of each cyclic SCC.
    Scc,
    /// Block one shortest cycle per cyclic SCC.
    #[default]
    Shortest,
}

impl CycleBlocking {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Scc => "scc",
            Self::Shortest => "shortest",
        }
    }
}

/// Shared SMT state and graph encoding for incremental solvers.
///
/// It owns the context, the node variables, the base graph constraints, and
/// the timeout/model-decoding rules. Solvers add their own objective (soft
/// clauses or a cost sum) and search loop on top.
pub(crate) struct SmtCore<'a> {
    pub(crate) g: &'a Graph,
    pub(crate) ctx: Context,
    pub(crate) vars: Vec<SExpr>,
    counter: usize,
    deadline: Option<Instant>,
}

impl<'a> SmtCore<'a> {
    pub(crate) fn new(g: &'a Graph, deadline: Option<Instant>) -> Result<Self, ExtractError> {
        check_deadline(deadline)?;
        let mut builder = easy_smt::ContextBuilder::new();
        if let Ok(z3_path) = std::env::var("Z3_PATH") {
            builder.solver(&z3_path);
            builder.solver_args(["-smt2", "-in", "-v:0"]);
        } else {
            builder.with_z3_defaults();
        }
        if std::env::var("SMT_DEBUG").is_ok() {
            builder.replay_file(Some(std::io::stderr()));
        }
        let ctx = builder
            .build()
            .map_err(|error| ExtractError::Other(error.into()))?;

        let mut r = Self {
            g,
            ctx,
            vars: vec![],
            counter: 0,
            deadline,
        };
        r.vars = g
            .graph
            .node_weights()
            .map(|n| r.new_bool(&n.label))
            .collect();

        assert!(!g.roots.is_empty());
        for root in g.roots.iter().copied() {
            r.ctx.assert(r.vars[root]).unwrap();
        }

        let filtered = NodeFiltered::from_fn(&g.graph, |node| g.used[node.index()]);
        for node in filtered.node_identifiers() {
            let var = filtered.node_weight(node).unwrap();
            let requirement = if !var.is_all {
                if filtered.neighbors(node).count() > 0 {
                    r.ctx
                        .or_many(filtered.neighbors(node).map(|ni| r.vars[ni.index()]))
                } else {
                    r.ctx.atoms().f
                }
            } else if filtered.neighbors(node).count() > 0 {
                r.ctx
                    .and_many(filtered.neighbors(node).map(|ni| r.vars[ni.index()]))
            } else {
                continue;
            };
            r.ctx
                .assert(r.ctx.imp(r.vars[node.index()], requirement))
                .unwrap();
        }
        Ok(r)
    }

    /// A selected non-root needs a selected predecessor. This removes
    /// disconnected model choices and makes the selected cost match the
    /// extracted index.
    pub(crate) fn assert_root_closure(&mut self) {
        let g = self.g;
        let filtered = NodeFiltered::from_fn(&g.graph, |node| g.used[node.index()]);
        for node in filtered.node_identifiers() {
            if g.roots.contains(&node.index()) {
                continue;
            }
            let predecessors: Vec<_> = filtered
                .neighbors_directed(node, petgraph::Direction::Incoming)
                .collect();
            let requirement = if predecessors.is_empty() {
                self.ctx.atoms().f
            } else {
                self.ctx
                    .or_many(predecessors.into_iter().map(|pred| self.vars[pred.index()]))
            };
            self.ctx
                .assert(self.ctx.imp(self.vars[node.index()], requirement))
                .unwrap();
        }
    }

    pub(crate) fn assert_total_cost(&mut self) -> SExpr {
        let terms: Vec<_> = self
            .g
            .graph
            .node_indices()
            .filter(|&node| self.g.used[node.index()])
            .filter_map(|node| {
                let cost = self.g.graph[node].cost;
                (cost > 0).then(|| {
                    self.ctx.ite(
                        self.vars[node.index()],
                        self.ctx.numeral(cost),
                        self.ctx.numeral(0),
                    )
                })
            })
            .collect();
        let cost = if terms.is_empty() {
            self.ctx.numeral(0)
        } else {
            self.ctx.plus_many(terms)
        };
        let total = self.new_var("_total_cost", self.ctx.int_sort());
        self.ctx.assert(self.ctx.eq(total, cost)).unwrap();
        total
    }

    /// At most one member per group, encoded with a prefix chain so a large
    /// group does not become quadratic. Unused members are skipped: their
    /// variables are free in this encoding and must not constrain the chain.
    pub(crate) fn assert_conflict_groups(&mut self) {
        for group in self.g.conflict_groups.iter() {
            let mut members = group.iter().copied().filter(|&var| self.g.used[var]);
            let Some(first) = members.next() else {
                continue;
            };
            let mut prefix_any = self.vars[first];
            for var in members {
                self.ctx
                    .assert(self.ctx.not(self.ctx.and(self.vars[var], prefix_any)))
                    .unwrap();
                let next = self.new_var("_cg", self.ctx.bool_sort());
                self.ctx
                    .assert(self.ctx.eq(next, self.ctx.or(prefix_any, self.vars[var])))
                    .unwrap();
                prefix_any = next;
            }
        }
    }

    pub(crate) fn new_bool(&mut self, label: &Option<String>) -> SExpr {
        let hint = match label {
            Some(s) => format!("_{}", s.replace("(", "_").replace(")", "_")),
            None => "".to_string(),
        };
        self.new_var(&hint, self.ctx.bool_sort())
    }

    pub(crate) fn new_var(&mut self, hint: &str, sort: SExpr) -> SExpr {
        self.counter += 1;
        let name = format!("v{}{hint}", self.counter);
        self.ctx.declare_const(name, sort).unwrap()
    }

    /// z3's timeout is per check, so refresh it from the actual remaining
    /// budget before every check. Setting it once at construction lets a late
    /// check spend the whole original budget past the deadline.
    fn refresh_timeout(&mut self) -> Result<(), ExtractError> {
        if let Some(timeout) = remaining(self.deadline)? {
            let millis = u64::try_from(timeout.as_millis())
                .unwrap_or(u64::MAX)
                .max(1);
            self.ctx
                .set_option(":timeout", self.ctx.numeral(millis))
                .map_err(|error| ExtractError::Other(error.into()))?;
        }
        Ok(())
    }

    pub(crate) fn check(&mut self, assumption: Option<SExpr>) -> Result<Check, ExtractError> {
        check_deadline(self.deadline)?;
        self.refresh_timeout()?;

        let response = match assumption {
            Some(assumption) => self.ctx.check_assuming([assumption]),
            None => self.ctx.check(),
        }
        .map_err(|error| ExtractError::Other(error.into()))?;

        match response {
            easy_smt::Response::Sat => {}
            easy_smt::Response::Unsat => return Ok(Check::Unsat),
            easy_smt::Response::Unknown if self.deadline.is_some() => {
                return Err(ExtractError::Timeout);
            }
            easy_smt::Response::Unknown => {
                return Err(ExtractError::Other(anyhow::Error::msg(
                    "solver returned unknown without a timeout",
                )));
            }
        }

        let values = self
            .ctx
            .get_value(self.vars.clone())
            .map_err(|error| ExtractError::Other(error.into()))?;
        let used = values
            .into_iter()
            .enumerate()
            .map(|(n, (_, value))| {
                if !self.g.used[n] || value == self.ctx.atoms().f {
                    false
                } else if value == self.ctx.atoms().t {
                    true
                } else {
                    panic!("unexpected {}", self.ctx.display(value))
                }
            })
            .collect();
        Ok(Check::Sat(used))
    }

    pub(crate) fn assert_cycles(&mut self, cycles: &[Vec<VertexId>]) {
        for cycle in cycles {
            let cycle = self.ctx.and_many(cycle.iter().map(|&var| self.vars[var]));
            self.ctx.assert(self.ctx.not(cycle)).unwrap();
        }
    }

    /// Block cyclic components in `selected` and return the blocked sets.
    /// `Scc` blocks every vertex of each SCC; `Shortest` blocks one shortest
    /// cycle per SCC. The returned sets are the clauses that were asserted.
    pub(crate) fn block_cycles(
        &mut self,
        selected: &[bool],
        blocking: CycleBlocking,
    ) -> Vec<Vec<VertexId>> {
        let components = self.g.detect_components(selected);
        let blocked = match blocking {
            CycleBlocking::Scc => components,
            CycleBlocking::Shortest => components
                .iter()
                .filter_map(|component| self.g.shortest_cycle(selected, component))
                .collect(),
        };
        self.assert_cycles(&blocked);
        blocked
    }

    pub(crate) fn block_4cycles(&mut self) -> usize {
        let cycles = self.g.detect_cycles_4();
        for cycle in &cycles {
            let cycle = self.ctx.and_many(cycle.iter().map(|&var| self.vars[var]));
            self.ctx.assert(self.ctx.not(cycle)).unwrap();
        }
        cycles.len()
    }
}
