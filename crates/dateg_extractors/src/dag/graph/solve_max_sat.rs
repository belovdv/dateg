use du_utils_timed::timed_print;
use easy_smt::{Context, SExpr};
use petgraph::{
    data::DataMap,
    visit::{IntoNeighbors, IntoNodeIdentifiers, NodeFiltered},
};

use super::Graph;

pub struct DagExtractor<'a> {
    g: &'a Graph,
    ctx: Context,
    vars: Vec<SExpr>,
    counter: usize,
}

impl<'a> DagExtractor<'a> {
    pub fn new(g: &'a Graph) -> Self {
        let ctx = timed_print("smt::Context::new", 100, || {
            let mut builder = easy_smt::ContextBuilder::new();
            if let Some(z3_path) = std::env::var("Z3_PATH").ok() {
                builder.solver(&z3_path);
                builder.solver_args(["-smt2", "-in", "-v:0"]);
            } else {
                builder.with_z3_defaults();
            }
            if std::env::var("SMT_DEBUG").is_ok() {
                builder.replay_file(Some(std::io::stderr()));
            }
            builder.build().unwrap()
        });

        let mut r = Self {
            g,
            ctx,
            vars: vec![],
            counter: 0,
        };
        timed_print("DagExtractor::init", 100, || {
            r.vars = g
                .graph
                .node_weights()
                .map(|n| r.new_var(&n.label))
                .collect();

            assert!(!g.roots.is_empty());
            for root in g.roots.iter().copied() {
                r.ctx.assert(r.vars[root]).unwrap();
            }

            let g = NodeFiltered::from_fn(&g.graph, |node| g.used[node.index()]);

            for node in g.node_identifiers() {
                let var = g.node_weight(node).unwrap();
                if var.cost > 0 {
                    let assert_soft = r.ctx.list(vec![
                        r.ctx.atom("assert-soft"),
                        r.ctx.not(r.vars[node.index()]),
                        r.ctx.atom(":weight"),
                        r.ctx.numeral(var.cost),
                    ]);
                    r.ctx.raw_send(assert_soft).unwrap();
                    r.ctx.raw_recv().unwrap();
                }

                let requirement = if !var.is_all {
                    if g.neighbors(node).count() > 0 {
                        r.ctx
                            .or_many(g.neighbors(node).map(|ni| r.vars[ni.index()]))
                    } else {
                        r.ctx.atoms().f
                    }
                } else if g.neighbors(node).count() > 0 {
                    r.ctx
                        .and_many(g.neighbors(node).map(|ni| r.vars[ni.index()]))
                } else {
                    continue;
                };
                r.ctx
                    .assert(r.ctx.imp(r.vars[node.index()], requirement))
                    .unwrap();
            }

            let s_cg = Some("cg".to_string());
            for group in r.g.conflict_groups.iter() {
                let mut prefix_any = r.vars[group[0]];
                for var in group.iter().skip(1).copied().filter(|&var| r.g.used[var]) {
                    r.ctx
                        .assert(r.ctx.not(r.ctx.and(r.vars[var], prefix_any)))
                        .unwrap();
                    let next = r.new_var(&s_cg);
                    r.ctx
                        .assert(r.ctx.eq(next, r.ctx.or(prefix_any, r.vars[var])))
                        .unwrap();
                    prefix_any = next;
                }
            }
        });
        r
    }

    fn new_var(&mut self, label: &Option<String>) -> SExpr {
        self.counter += 1;
        let hint = match label {
            Some(s) => format!("_{}", s.replace("(", "_").replace(")", "_")),
            None => "".to_string(),
        };
        let name = format!("v{}{hint}", self.counter);
        self.ctx.declare_const(name, self.ctx.atoms().bool).unwrap()
    }

    /// Increment for search
    ///
    ///  Expects that solution actually exists
    fn try_solve(&mut self) -> Option<Vec<bool>> {
        match self.ctx.check().unwrap() {
            easy_smt::Response::Sat => {}
            easy_smt::Response::Unsat => panic!("unsat"),
            easy_smt::Response::Unknown => panic!("unknown"),
        }

        let values = self.ctx.get_value(self.vars.clone()).unwrap();
        let used: Vec<_> = values
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

        let cycles = self.g.detect_components(&used);
        if cycles.is_empty() {
            return Some(used);
        }
        log::debug!("cycles.len {}", cycles.len());

        // Note: it might be beneficial to only block the shortest cycles.
        for cycle in cycles {
            let cycle = self.ctx.and_many(cycle.iter().map(|&var| self.vars[var]));
            self.ctx.assert(self.ctx.not(cycle)).unwrap();
        }

        None
    }

    pub fn solve(&mut self) -> Vec<bool> {
        for cycle in self.g.detect_cycles_4() {
            let cycle = self.ctx.and_many(cycle.iter().map(|&var| self.vars[var]));
            self.ctx.assert(self.ctx.not(cycle)).unwrap();
        }

        let bound = match std::env::var("DATEG_GRAPH_SOLVE_BOUND") {
            Ok(v) => v.parse().unwrap(),
            Err(_) => usize::MAX,
        };
        for i in 0..bound {
            if let Some(solution) =
                timed_print(format!("DagExtractor::step {i}"), 100, || self.try_solve())
            {
                return solution;
            }
        }
        panic!("out of bound on iterations ({bound})")
    }
}
