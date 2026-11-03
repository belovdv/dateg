use std::time::Instant;

use easy_smt::{Context, SExpr};
use petgraph::{
    data::DataMap,
    visit::{IntoNeighbors, IntoNeighborsDirected, IntoNodeIdentifiers, NodeFiltered},
};

use super::super::cycles;
use super::super::{
    CycleBlocking, Graph, PreemptiveCyclesConfig, VertexId, View, assignment_cost, reachable,
};
use crate::dag::{Error, check_deadline, remaining};

pub(crate) enum Check {
    Sat(Vec<bool>),
    Unsat,
}

// Solver failures are `io::Error`s carrying z3's `(error "...")` reply, so
// classify them once here instead of at every call site.
//
// Not ideal: the impl is crate-wide, so any non-solver `io::Error` in the
// extractor would also become a solver error (`Custom`). That is acceptable
// while the SMT context is the only I/O source, and it keeps the encoding
// code free of `map_err` noise.
//
// `canceled` is z3's timeout/resource-exhaustion word, not only user
// cancellation: see `common_msgs::g_canceled_msg` (`src/util/common_msgs.cpp`),
// `CANCELED -> "canceled"` in `src/smt/smt_context_pp.cpp`, and the note in
// `src/opt/optsmt.cpp` that it is used for resource exhaustion. The SMT2
// frontend prints it as `(error "line N column M: canceled")`
// (`src/parsers/smt2/smt2parser.cpp`). z3 itself matches both spellings
// (`src/solver/parallel_tactical.cpp`), hence both are accepted here.
impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        let message = error.to_string().to_ascii_lowercase();
        if message.contains("out of memory") {
            Self::OutOfMemory
        } else if message.contains("canceled")
            || message.contains("cancelled")
            || message.contains("timeout")
        {
            Self::Timeout
        } else {
            Self::Custom(error.to_string())
        }
    }
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
    pub(crate) roots: &'a [VertexId],
    pub(crate) view: &'a View,
    /// Root-reachable enabled vertices, derived for this encoding.
    pub(crate) domain: Vec<bool>,
    pub(crate) ctx: Context,
    pub(crate) vars: Vec<SExpr>,
    counter: usize,
    deadline: Option<Instant>,
}

impl<'a> SmtCore<'a> {
    pub(crate) fn new(
        g: &'a Graph,
        roots: &'a [VertexId],
        view: &'a View,
        deadline: Option<Instant>,
    ) -> Result<Self, Error> {
        check_deadline(deadline)?;
        let mut builder = easy_smt::ContextBuilder::new();
        let mut args = vec!["-smt2".to_string(), "-in".to_string(), "-v:0".to_string()];
        // A per-process cap, so a runaway query degrades to the cheaper
        // fallback instead of taking the whole run down. `0` disables it.
        let memory_mb = std::env::var("Z3_MEMORY_MB")
            .ok()
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(512);
        args.push(format!("-memory:{memory_mb}"));
        if let Ok(z3_path) = std::env::var("Z3_PATH") {
            builder.solver(z3_path);
        } else {
            builder.solver("z3");
        }
        builder.solver_args(args);
        if std::env::var("SMT_DEBUG").is_ok() {
            builder.replay_file(Some(std::io::stderr()));
        }
        let ctx = builder.build()?;

        let domain = reachable(g, roots, view);
        let mut r = Self {
            g,
            roots,
            view,
            domain,
            ctx,
            vars: vec![],
            counter: 0,
            deadline,
        };
        r.vars = g
            .graph
            .node_weights()
            .map(|n| r.new_bool(&n.label))
            .collect::<Result<Vec<_>, _>>()?;

        assert!(!roots.is_empty());
        for &root in roots {
            r.ctx.assert(r.vars[root])?;
        }

        let filtered = NodeFiltered::from_fn(&g.graph, |node| r.domain[node.index()]);
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
            r.ctx.assert(r.ctx.imp(r.vars[node.index()], requirement))?;
        }
        Ok(r)
    }

    /// A selected non-root needs a selected predecessor. This removes
    /// disconnected model choices and makes the selected cost match the
    /// extracted index.
    pub(crate) fn assert_root_closure(&mut self) -> Result<(), Error> {
        let g = self.g;
        let filtered = NodeFiltered::from_fn(&g.graph, |node| self.domain[node.index()]);
        for node in filtered.node_identifiers() {
            if self.roots.contains(&node.index()) {
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
                .assert(self.ctx.imp(self.vars[node.index()], requirement))?;
        }
        Ok(())
    }

    pub(crate) fn assert_total_cost(&mut self) -> Result<SExpr, Error> {
        let terms: Vec<_> = self
            .g
            .graph
            .node_indices()
            .filter(|&node| self.domain[node.index()])
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
        let total = self.new_var("_total_cost", self.ctx.int_sort())?;
        self.ctx.assert(self.ctx.eq(total, cost))?;
        Ok(total)
    }

    pub(crate) fn cost(&self, assignment: &[bool]) -> usize {
        assignment_cost(self.g, self.view, assignment)
    }

    /// At most one member per group, encoded with a prefix chain so a large
    /// group does not become quadratic. Out-of-domain members are skipped:
    /// their variables are free in this encoding and must not constrain the
    /// chain.
    pub(crate) fn assert_conflict_groups(&mut self) -> Result<(), Error> {
        for group in self.view.all_groups(self.g) {
            let members: Vec<_> = group
                .iter()
                .copied()
                .filter(|&var| self.domain[var])
                .collect();
            let Some(&first) = members.first() else {
                continue;
            };
            let mut prefix_any = self.vars[first];
            for &var in &members[1..] {
                self.ctx
                    .assert(self.ctx.not(self.ctx.and(self.vars[var], prefix_any)))?;
                let next = self.new_var("_cg", self.ctx.bool_sort())?;
                self.ctx
                    .assert(self.ctx.eq(next, self.ctx.or(prefix_any, self.vars[var])))?;
                prefix_any = next;
            }
        }
        Ok(())
    }

    pub(crate) fn new_bool(&mut self, label: &Option<String>) -> Result<SExpr, Error> {
        let hint = match label {
            Some(s) => format!("_{}", s.replace("(", "_").replace(")", "_")),
            None => "".to_string(),
        };
        self.new_var(&hint, self.ctx.bool_sort())
    }

    pub(crate) fn new_var(&mut self, hint: &str, sort: SExpr) -> Result<SExpr, Error> {
        self.counter += 1;
        let name = format!("v{}{hint}", self.counter);
        Ok(self.ctx.declare_const(name, sort)?)
    }

    /// z3's timeout is per check, so refresh it from the actual remaining
    /// budget before every check. Setting it once at construction lets a late
    /// check spend the whole original budget past the deadline.
    fn refresh_timeout(&mut self) -> Result<(), Error> {
        if let Some(timeout) = remaining(self.deadline)? {
            let millis = u64::try_from(timeout.as_millis())
                .unwrap_or(u64::MAX)
                .max(1);
            self.ctx.set_option(":timeout", self.ctx.numeral(millis))?;
        }
        Ok(())
    }

    /// z3 reports a canceled check as `(error "line ... canceled")`, which the
    /// `From` impl maps to `Timeout`. Errors that do not say so are still
    /// timeouts once the deadline has passed, so promote them here rather than
    /// at every solver call.
    fn check_error(&self, error: std::io::Error) -> Error {
        match (Error::from(error), self.deadline) {
            (Error::Custom(_), Some(deadline)) if Instant::now() >= deadline => Error::Timeout,
            (error, _) => error,
        }
    }

    pub(crate) fn check(&mut self, assumption: Option<SExpr>) -> Result<Check, Error> {
        check_deadline(self.deadline)?;
        self.refresh_timeout()?;

        let response = match assumption {
            Some(assumption) => self.ctx.check_assuming([assumption]),
            None => self.ctx.check(),
        }
        .map_err(|error| self.check_error(error))?;

        match response {
            easy_smt::Response::Sat => {}
            easy_smt::Response::Unsat => return Ok(Check::Unsat),
            easy_smt::Response::Unknown if self.deadline.is_some() => {
                return Err(Error::Timeout);
            }
            easy_smt::Response::Unknown => {
                return Err(Error::Custom(
                    "solver returned unknown without a timeout".into(),
                ));
            }
        }

        let values = self
            .ctx
            .get_value(self.vars.clone())
            .map_err(|error| self.check_error(error))?;
        let selected = values
            .into_iter()
            .enumerate()
            .map(|(n, (_, value))| {
                if !self.domain[n] || value == self.ctx.atoms().f {
                    false
                } else if value == self.ctx.atoms().t {
                    true
                } else {
                    panic!("unexpected {}", self.ctx.display(value))
                }
            })
            .collect();
        Ok(Check::Sat(selected))
    }

    pub(crate) fn assert_cycles(&mut self, cycles: &[Vec<VertexId>]) -> Result<(), Error> {
        for cycle in cycles {
            let cycle = self.ctx.and_many(cycle.iter().map(|&var| self.vars[var]));
            self.ctx.assert(self.ctx.not(cycle))?;
        }
        Ok(())
    }

    /// Block cyclic components in `selected` and return the blocked sets.
    /// `Scc` blocks every vertex of each SCC; `Shortest` blocks one shortest
    /// cycle per SCC. The returned sets are the clauses that were asserted.
    pub(crate) fn block_cycles(
        &mut self,
        selected: &[bool],
        blocking: CycleBlocking,
    ) -> Result<Vec<Vec<VertexId>>, Error> {
        let components = self.g.detect_components(selected);
        let blocked = match blocking {
            CycleBlocking::Scc => components,
            CycleBlocking::Shortest => components
                .iter()
                .filter_map(|component| self.g.shortest_cycle(selected, component))
                .collect(),
        };
        self.assert_cycles(&blocked)?;
        Ok(blocked)
    }

    /// Assert one clause per class cycle up to the configured length. Each
    /// clause rules out all node cycles with that class sequence. A missing
    /// length uses the auto ceiling; missing caps use the reachable cycle rank
    /// and `10 *` reachable vertices.
    pub(crate) fn assert_preemptive_cycles(
        &mut self,
        config: &PreemptiveCyclesConfig,
    ) -> Result<cycles::ClassCycleEnumeration, Error> {
        let vertices = self.domain.iter().filter(|&&active| active).count();
        let (rank, _, cyclic) = self.g.cycle_analysis(&self.domain);
        let max_length = config.max_length.unwrap_or(cycles::AUTO_MAX_LENGTH);
        let max_cycles = config
            .max_cycles
            .unwrap_or_else(|| usize::try_from(rank).unwrap_or(usize::MAX));
        let max_literals = config
            .max_literals
            .unwrap_or_else(|| vertices.saturating_mul(10));
        let deadline = self.deadline;
        let ctx = &mut self.ctx;
        let vars = &self.vars;
        let mut clauses = Vec::new();
        let report = cycles::for_each_class_cycle(
            self.g,
            &cyclic,
            max_length,
            max_cycles,
            max_literals,
            deadline,
            &mut |_classes, members| {
                let disjuncts: Vec<_> = members
                    .iter()
                    .map(|members| ctx.and_many(members.iter().map(|&node| ctx.not(vars[node]))))
                    .collect();
                clauses.push(ctx.or_many(disjuncts));
            },
        );
        for clause in clauses {
            ctx.assert(clause)?;
        }
        Ok(report)
    }
}
