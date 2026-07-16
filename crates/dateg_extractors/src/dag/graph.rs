use std::time::Duration;

use du_utils_timed::{timed, timed_print};
use easy_smt::{Context, SExpr};

#[derive(Default)]
pub struct Graph {
    root: Option<VertexId>,
    vars: Vec<Vertex>,
    used: Vec<bool>,
    conflict_groups: Vec<Vec<VertexId>>,
}
pub type VertexId = usize;
struct Vertex {
    is_all: bool,
    cost: usize,
    args: Vec<VertexId>,
}

impl Graph {
    pub fn set_root(&mut self, root: VertexId) {
        assert!(self.root.is_none());
        self.root = Some(root);
    }

    pub fn add_vertex(&mut self, is_all: bool, cost: usize) -> VertexId {
        let id = self.vars.len();
        let args = vec![];
        self.vars.push(Vertex { is_all, cost, args });
        id
    }
    pub fn add_vertex_args(&mut self, vertex: VertexId, args: impl IntoIterator<Item = VertexId>) {
        self.vars[vertex].args.extend(args);
    }

    pub fn add_conflicting_group(&mut self, group: impl IntoIterator<Item = VertexId>) {
        let group: Vec<_> = group.into_iter().collect();
        assert!(!group.is_empty());
        if group.len() > 1 {
            self.conflict_groups.push(group);
        }
    }

    pub fn solve(self) -> Vec<bool> {
        DagExtractor::new(self).solve()
    }

    fn collect_usage(&mut self) {
        assert!(self.used.is_empty());
        self.used = vec![false; self.vars.len()];
        let mut queue = vec![self.root.unwrap()];
        while let Some(next) = queue.pop() {
            if self.used[next] {
                continue;
            }
            self.used[next] = true;
            for arg in self.vars[next].args.iter().copied() {
                queue.push(arg);
            }
        }
    }

    /// Expects Z3
    fn initialize_context(&self, ctx: &mut Context) -> Vec<SExpr> {
        let mut counter = 0;
        macro_rules! var {
            () => {{
                counter += 1;
                ctx.declare_const(format!("v{counter}"), ctx.atoms().bool)
                    .unwrap()
            }};
        }
        let v: Vec<_> = (0..self.vars.len()).map(|_| var!()).collect();

        ctx.assert(v[self.root.unwrap()]).unwrap();
        for (n, var) in self.vars.iter().enumerate() {
            if !self.used[n] {
                continue;
            }

            if var.cost > 0 {
                let assert_soft = ctx.list(vec![
                    ctx.atom("assert-soft"),
                    ctx.not(v[n]),
                    ctx.atom(":weight"),
                    ctx.numeral(var.cost),
                ]);
                ctx.raw_send(assert_soft).unwrap();
                ctx.raw_recv().unwrap();
            }

            let requirement = if !var.is_all {
                assert!(!var.args.is_empty());
                ctx.or_many(var.args.iter().map(|&arg| v[arg]))
            } else if !var.args.is_empty() {
                ctx.and_many(var.args.iter().map(|&arg| v[arg]))
            } else {
                continue;
            };
            ctx.assert(ctx.imp(v[n], requirement)).unwrap();
        }

        for group in self.conflict_groups.iter() {
            let mut prefix_any = v[group[0]];
            for var in group.iter().skip(1).copied() {
                ctx.assert(ctx.not(ctx.and(v[var], prefix_any))).unwrap();
                let next = var!();
                ctx.assert(ctx.eq(next, ctx.or(prefix_any, v[var])))
                    .unwrap();
                prefix_any = next;
            }
        }

        v
    }

    fn detect_cycles(&self, used: &[bool]) -> Vec<Vec<VertexId>> {
        let n = self.vars.len();
        let mut state = vec![0; n]; // 0=unvisited, 1=visiting, 2=done
        let mut cycles = vec![];
        let mut stack = vec![];

        fn dfs(
            v: VertexId,
            graph: &Graph,
            used: &[bool],
            state: &mut Vec<u8>,
            stack: &mut Vec<VertexId>,
            cycles: &mut Vec<Vec<VertexId>>,
        ) {
            state[v] = 1;
            stack.push(v);
            for &to in graph.vars[v].args.iter().filter(|&&to| used[to]) {
                if state[to] == 0 {
                    dfs(to, graph, used, state, stack, cycles);
                } else if state[to] == 1 {
                    let pos = stack.iter().position(|&x| x == to).unwrap();
                    cycles.push(stack[pos..].to_vec());
                }
            }
            state[v] = 2;
            stack.pop();
        }

        for start in 0..n {
            if used[start] && state[start] == 0 {
                dfs(start, self, used, &mut state, &mut stack, &mut cycles);
            }
        }
        cycles
    }
}

struct DagExtractor {
    ctx: Context,
    dag: Graph,
    vars: Vec<SExpr>,

    pub times: Vec<Duration>,
}

impl DagExtractor {
    fn new(mut g: Graph) -> Self {
        g.collect_usage();
        let (ctx, vars) = timed_print("DagExtractor::new", 100, || {
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
            let mut ctx = builder.build().unwrap();

            let vars = g.initialize_context(&mut ctx);
            (ctx, vars)
        });
        Self {
            ctx,
            dag: g,
            vars,
            times: vec![],
        }
    }

    /// Increment for search
    ///
    ///  Expects that solution actually exists
    fn try_solve(&mut self) -> Option<Vec<bool>> {
        let (time, r) = timed(|| self.ctx.check());
        self.times.push(time);
        match r.unwrap() {
            easy_smt::Response::Sat => {}
            easy_smt::Response::Unsat => panic!("unsat"),
            easy_smt::Response::Unknown => panic!("unknown"),
        }

        let values = self.ctx.get_value(self.vars.clone()).unwrap();
        let used: Vec<_> = values
            .into_iter()
            .enumerate()
            .map(|(n, (_, value))| {
                if !self.dag.used[n] || value == self.ctx.atoms().f {
                    false
                } else if value == self.ctx.atoms().t {
                    true
                } else {
                    panic!("unexpected {}", self.ctx.display(value))
                }
            })
            .collect();

        let cycles = self.dag.detect_cycles(&used);
        if cycles.is_empty() {
            return Some(used);
        }

        for cycle in cycles {
            let cycle = self.ctx.and_many(cycle.iter().map(|&var| self.vars[var]));
            self.ctx.assert(self.ctx.not(cycle)).unwrap();
        }

        None
    }

    fn solve(&mut self) -> Vec<bool> {
        loop {
            if let Some(solution) = timed_print("DagExtractor::step", 1000, || self.try_solve()) {
                return solution;
            }
        }
    }
}

#[test]
fn dag_extractor() {
    let mut g = Graph::default();
    let root = g.add_vertex(true, 1);
    let a = g.add_vertex(true, 1);
    let b = g.add_vertex(false, 1);
    let c = g.add_vertex(true, 1);
    let d = g.add_vertex(true, 1);
    g.set_root(root);
    g.add_vertex_args(root, [a, b]);
    g.add_vertex_args(b, [c, d]);
    let assignment = DagExtractor::new(g).solve();
    assert!(
        vec![true, true, true, false, true] == assignment
            || vec![true, true, true, true, false] == assignment
    );

    let mut g = Graph::default();
    let root = g.add_vertex(true, 0);
    let a = g.add_vertex(false, 1);
    let b = g.add_vertex(false, 1);
    let c = g.add_vertex(false, 1);
    let d = g.add_vertex(true, 1);
    let e = g.add_vertex(true, 1);
    g.set_root(root);
    g.add_vertex_args(root, [a, c]);
    g.add_vertex_args(a, [b, d]);
    g.add_vertex_args(b, [a, c]);
    g.add_vertex_args(c, [b, e]);
    let assignment = DagExtractor::new(g).solve();
    assert_eq!(assignment, vec![true, true, false, true, true, true]);

    let mut g = Graph::default();
    let root = g.add_vertex(true, 0);
    let x = g.add_vertex(false, 100);
    let y = g.add_vertex(true, 0);
    g.set_root(root);
    g.add_vertex_args(x, [y]);
    let assignment = DagExtractor::new(g).solve();
    assert_eq!(assignment, vec![true, false, false]);

    let mut g = Graph::default();
    let root = g.add_vertex(false, 0);
    let a = g.add_vertex(true, 1);
    let b = g.add_vertex(true, 0);
    let c = g.add_vertex(true, 0);
    g.set_root(root);
    g.add_vertex_args(root, [a, b, c]);
    g.add_conflicting_group([a, b, c]);
    let mut extractor = DagExtractor::new(g);
    let solution = extractor.solve();
    assert!(
        solution == vec![true, false, true, false] || solution == vec![true, false, false, true]
    );
}
