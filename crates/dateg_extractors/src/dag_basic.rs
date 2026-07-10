use crate::index::{Constructor, IndexFor};
use ahash::AHashMap;
use dateg::{EGraph, FunctionId, Schema, Table, Token, TokenTuple, Tuple, Value};
use du_utils_timed::timed_print;
use easy_smt::{Context, SExpr};
use std::{any::TypeId, marker::PhantomData};

#[derive(Default)]
pub struct ExtractorDAG<Index> {
    _p: PhantomData<Index>,
    ctx: Option<Context>,

    /// Token -> TokenValue -> smt self var and constructors vars
    values: AHashMap<(TypeId, Value), ValueData>,
    /// table function -> inputs -> token value (output), var (self)
    constructors: AHashMap<FunctionId, AHashMap<Vec<(TypeId, Value)>, ((TypeId, Value), SExpr)>>,
    /// table function -> cost
    constructors_cost: AHashMap<FunctionId, usize>,

    constructors_init: Vec<Box<dyn Fn(&mut Self, &mut CTX, &EGraph)>>,
    constructors_collect: Vec<Box<dyn Fn(&Self, &mut Index, &mut Context)>>,
}

struct ValueData {
    var_value: SExpr,
    vars_constructors: Vec<SExpr>,
}
impl ValueData {
    fn new(var_value: SExpr) -> Self {
        Self {
            var_value,
            vars_constructors: vec![],
        }
    }
}

impl<Index: Default> ExtractorDAG<Index> {
    pub fn extract<Tok: Token>(&mut self, token: Tok) -> anyhow::Result<Index> {
        let mut ctx = std::mem::take(&mut self.ctx).unwrap();
        ctx.push().unwrap();

        let var = self.values[&(TypeId::of::<Tok>(), token.into_value())].var_value;
        ctx.assert(var).unwrap();

        match timed_print("check", 1000, || ctx.check()).unwrap() {
            easy_smt::Response::Sat => {}
            easy_smt::Response::Unsat => anyhow::bail!("unsat"),
            easy_smt::Response::Unknown => anyhow::bail!("unknown"),
        };

        let mut index = Index::default();
        for constructor in self.constructors_collect.iter() {
            constructor(self, &mut index, &mut ctx);
        }

        ctx.pop().unwrap();
        self.ctx = Some(ctx);

        Ok(index)
    }

    pub fn init(&mut self, eg: &EGraph) {
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
        let ctx = builder.build().unwrap();
        let mut ctx = CTX {
            sort: ctx.atoms().bool,
            inner: ctx,
            counter: 0,
        };

        for collect in std::mem::take(&mut self.constructors_init) {
            collect(self, &mut ctx, eg);
        }
        let mut ctx = ctx.inner;

        for vd in self.values.values() {
            let cst = ctx.imp(
                vd.var_value,
                ctx.or_many(vd.vars_constructors.iter().copied()),
            );
            ctx.assert(cst).unwrap();
        }
        for (inputs, (_, var_construct)) in self.constructors.values().flat_map(|m| m.iter()) {
            if inputs.len() > 0 {
                let all_inputs =
                    ctx.and_many(inputs.iter().map(|input| self.values[input].var_value));
                let cst = ctx.imp(*var_construct, all_inputs);
                ctx.assert(cst).unwrap();
            }
        }

        for (table, constructors) in self.constructors.iter() {
            let cost = self.constructors_cost[table];
            for (_, var_construct) in constructors.values() {
                let weight = ctx.numeral(cost);
                let assert_soft_cmd = ctx.list(vec![
                    ctx.atom("assert-soft"),
                    ctx.not(*var_construct),
                    ctx.atom(":weight"),
                    weight,
                ]);
                ctx.raw_send(assert_soft_cmd).unwrap();
                ctx.raw_recv().unwrap();
            }
        }

        self.ctx = Some(ctx);
    }

    pub fn register_constructor<C: Constructor, S>(&mut self, table: Table<S>, cost: usize)
    where
        Index: IndexFor<C::Output, Sort = C::Sort>,
        S: Schema<Inputs = C::Inputs, Output = C::Output>,
    {
        self.constructors_cost.insert(table.egglog(), cost);
        self.constructors_init
            .push(Box::new(move |extractor, ctx, eg| {
                let constructors = extractor.constructors.entry(table.egglog()).or_default();
                eg.for_each_row(table, |inputs, output| {
                    let inputs = inputs.into_values();
                    if inputs.contains(&output.into_value()) {
                        return;
                    }
                    let vd = extractor
                        .values
                        .entry((TypeId::of::<S::Output>(), output.into_value()))
                        .or_insert_with(|| ValueData::new(ctx.gen_new_var("v")));
                    let input_tokens = S::Inputs::type_ids().zip(inputs);
                    let constructor = constructors
                        .entry(input_tokens.collect())
                        .or_insert_with(|| {
                            (
                                (TypeId::of::<S::Output>(), output.into_value()),
                                ctx.gen_new_var("c"),
                            )
                        })
                        .1;
                    vd.vars_constructors.push(constructor);
                });
            }));
        self.constructors_collect
            .push(Box::new(move |extractor, index, ctx| {
                let constructor = &extractor.constructors[&table.egglog()];
                let (constructor, usage): (Vec<_>, Vec<_>) = constructor
                    .iter()
                    .map(|(inputs, (output, var))| ((inputs.clone(), *output), *var))
                    .unzip();
                let values = ctx.get_value(usage).unwrap();
                for ((name, value), (inputs, output)) in values.iter().zip(constructor) {
                    let used = if *value == ctx.atoms().t {
                        true
                    } else if *value == ctx.atoms().f {
                        false
                    } else {
                        panic!("unknown {}: {}", ctx.display(*name), ctx.display(*value))
                    };
                    if used {
                        let inputs: Vec<_> = inputs.iter().map(|(_, value)| *value).collect();
                        let was = index.get_map_mut().insert(
                            S::Output::from_value(output.1),
                            (0, vec![C::into_variant(S::Inputs::from_values(&inputs))]),
                        );
                        assert!(was.is_none());
                    }
                }
            }));
    }
}

struct CTX {
    inner: Context,
    sort: SExpr,
    counter: usize,
}
impl CTX {
    fn gen_new_var(&mut self, hint: &str) -> SExpr {
        self.counter += 1;
        let name = format!("{hint}{}", self.counter);
        self.inner.declare_const(name, self.sort).unwrap()
    }
}
