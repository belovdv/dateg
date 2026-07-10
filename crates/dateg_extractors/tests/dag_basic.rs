mod nat;

use ahash::AHashMap;
use dateg::execute;
use du_utils_timed::timed_print;
use easy_smt::SExpr;

use crate::nat::{TokenExpr, get_val};

#[test]
fn dag_basic() {
    let mut eg = nat::eg();
    execute! {eg;
        (constructor inc (TokenExpr) TokenExpr)
        (constructor mul (TokenExpr TokenExpr) TokenExpr)
        (constructor one () TokenExpr)
        (constructor add (TokenExpr TokenExpr) TokenExpr)
        (constructor square (TokenExpr) TokenExpr)

        (= e1 (one))
        (= e2 (inc e1))
        (= e4 (square e2))
        (= e16 (square e4))
    }
    while eg.run_ruleset("") {}

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

    let mut counter = 0;
    let mut gen_tmp_name = |hint: &str| {
        counter += 1;
        format!("{hint}{counter}")
    };
    let bool_sort = ctx.bool_sort();

    let mut values_expr: AHashMap<TokenExpr, SExpr> = Default::default();
    let mut constructors_expr: AHashMap<TokenExpr, Vec<SExpr>> = Default::default();

    let mut values_one: AHashMap<(), (TokenExpr, SExpr)> = Default::default();
    let mut values_inc: AHashMap<(TokenExpr,), (TokenExpr, SExpr)> = Default::default();
    let mut values_mul: AHashMap<(TokenExpr, TokenExpr), (TokenExpr, SExpr)> = Default::default();

    eg.for_each_row(one, |inputs, output| {
        values_expr
            .entry(output)
            .or_insert_with(|| ctx.declare_const(gen_tmp_name("expr"), bool_sort).unwrap());
        let constructor = values_one
            .entry(inputs)
            .or_insert_with(|| {
                (
                    output,
                    ctx.declare_const(gen_tmp_name("one"), bool_sort).unwrap(),
                )
            })
            .1;
        constructors_expr
            .entry(output)
            .or_default()
            .push(constructor);
    });
    eg.for_each_row(inc, |inputs, output| {
        if inputs.0 == output {
            return;
        }
        values_expr
            .entry(output)
            .or_insert_with(|| ctx.declare_const(gen_tmp_name("expr"), bool_sort).unwrap());
        let constructor = values_inc
            .entry(inputs)
            .or_insert_with(|| {
                (
                    output,
                    ctx.declare_const(gen_tmp_name("inc"), bool_sort).unwrap(),
                )
            })
            .1;
        constructors_expr
            .entry(output)
            .or_default()
            .push(constructor);
    });
    eg.for_each_row(mul, |inputs, output| {
        if inputs.0 == output {
            return;
        }
        if inputs.1 == output {
            return;
        }
        values_expr
            .entry(output)
            .or_insert_with(|| ctx.declare_const(gen_tmp_name("expr"), bool_sort).unwrap());
        let constructor = values_mul
            .entry(inputs)
            .or_insert_with(|| {
                (
                    output,
                    ctx.declare_const(gen_tmp_name("mul"), bool_sort).unwrap(),
                )
            })
            .1;
        constructors_expr
            .entry(output)
            .or_default()
            .push(constructor);
    });

    for (value, constructors) in constructors_expr.iter() {
        let value = values_expr[value];
        let cst = ctx.imp(value, ctx.or_many(constructors.iter().copied()));
        ctx.assert(cst).unwrap();
    }
    // for ((), constructor) in values_one.iter() {
    //     let all_inputs = ctx.and_many([].iter().copied());
    //     let cst = ctx.imp(*constructor, all_inputs);
    //     ctx.assert(cst).unwrap();
    // }
    for ((a,), constructor) in values_inc.iter() {
        let all_inputs = ctx.and_many([values_expr[a]].iter().copied());
        let cst = ctx.imp(constructor.1, all_inputs);
        ctx.assert(cst).unwrap();
    }
    for ((a, b), constructor) in values_mul.iter() {
        let all_inputs = ctx.and_many([values_expr[a], values_expr[b]].iter().copied());
        let cst = ctx.imp(constructor.1, all_inputs);
        ctx.assert(cst).unwrap();
    }

    let weight = ctx.numeral(1);
    for v in values_one
        .values()
        .chain(values_inc.values())
        .chain(values_mul.values())
        .map(|(_, var)| *var)
    {
        let assert_soft_cmd = ctx.list(vec![
            ctx.atom("assert-soft"),
            ctx.not(v),
            ctx.atom(":weight"),
            weight,
        ]);
        ctx.raw_send(assert_soft_cmd).unwrap();
        ctx.raw_recv().unwrap();
    }

    let e4 = get_val(&mut eg, 4);
    let e13 = get_val(&mut eg, 13);
    for (e, expected) in [
        (e4, &["(mul (inc (one)) (inc (one)))"][..]),
        (
            e13,
            &[
                "(inc (mul (inc (inc (inc (one)))) (inc (inc (one)))))",
                // +1(* 2 *32)
                "(inc (mul (inc (one)) (mul (inc (inc (one))) (inc (one)))))",
                // +1(* 3 *22)
                "(inc (mul (inc (inc (one))) (mul (inc (one)) (inc (one)))))",
            ],
        ),
        (
            e16,
            &["(mul (mul (inc (one)) (inc (one))) (mul (inc (one)) (inc (one))))"],
        ),
    ] {
        ctx.push().unwrap();

        let e_enabled = values_expr[&e];
        ctx.assert(e_enabled).unwrap();

        match timed_print("check", 1000, || ctx.check()).unwrap() {
            easy_smt::Response::Sat => {}
            easy_smt::Response::Unsat => panic!("unsat"),
            easy_smt::Response::Unknown => panic!("unknown"),
        };

        let mut index = Index::default();

        let (constructor_one, usage_one): (Vec<_>, Vec<_>) = values_one
            .iter()
            .map(|(&inputs, &(output, var))| ((inputs, output), var))
            .unzip();
        let values = ctx.get_value(usage_one).unwrap();
        for ((name, value), (inputs, output)) in values.iter().zip(constructor_one.iter()) {
            let used = if *value == ctx.atoms().t {
                true
            } else if *value == ctx.atoms().f {
                false
            } else {
                panic!("unknown {}: {}", ctx.display(*name), ctx.display(*value))
            };

            if used {
                let was = index.expr.insert(*output, Expr::One(*inputs));
                assert!(was.is_none());
            }
        }

        let (constructor_inc, usage_inc): (Vec<_>, Vec<_>) = values_inc
            .iter()
            .map(|(&inputs, &(output, var))| ((inputs, output), var))
            .unzip();
        let values = ctx.get_value(usage_inc).unwrap();
        for ((name, value), (inputs, output)) in values.iter().zip(constructor_inc.iter()) {
            let used = if *value == ctx.atoms().t {
                true
            } else if *value == ctx.atoms().f {
                false
            } else {
                panic!("unknown {}: {}", ctx.display(*name), ctx.display(*value))
            };

            if used {
                let was = index.expr.insert(*output, Expr::Inc(*inputs));
                assert!(was.is_none());
            }
        }

        let (constructor_mul, usage_mul): (Vec<_>, Vec<_>) = values_mul
            .iter()
            .map(|(&inputs, &(output, var))| ((inputs, output), var))
            .unzip();
        let values = ctx.get_value(usage_mul).unwrap();
        for ((name, value), (inputs, output)) in values.iter().zip(constructor_mul.iter()) {
            let used = if *value == ctx.atoms().t {
                true
            } else if *value == ctx.atoms().f {
                false
            } else {
                panic!("unknown {}: {}", ctx.display(*name), ctx.display(*value))
            };

            if used {
                let was = index.expr.insert(*output, Expr::Mul(*inputs));
                assert!(was.is_none(), "{was:?}");
            }
        }

        let got = index.expr_to_string(&index.expr[&e]);
        assert!(expected.contains(&got.as_str()), "{expected:#?}    {got}");

        ctx.pop().unwrap();
    }
}

#[derive(Default)]
pub struct Index {
    pub expr: dateg_extractors::AHashMap<TokenExpr, Expr>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Expr {
    One(()),
    Inc((TokenExpr,)),
    Mul((TokenExpr, TokenExpr)),
}

impl Index {
    fn expr_to_string(&self, expr: &Expr) -> String {
        match expr {
            Expr::One(()) => format!("(one)"),
            Expr::Inc((a,)) => format!("(inc {})", self.expr_to_string(&self.expr[a])),
            Expr::Mul((a, b)) => {
                let mut a = self.expr_to_string(&self.expr[a]);
                let mut b = self.expr_to_string(&self.expr[b]);
                // To simplify testing
                if a > b {
                    std::mem::swap(&mut a, &mut b);
                }
                format!("(mul {a} {b})")
            }
        }
    }
}
