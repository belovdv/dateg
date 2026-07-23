mod nat;

use dateg::{EGraph, TokenOpaque, execute};

use dateg_extractors::dag::index_dag;
use nat::*;

#[test]
fn dag_basics() {
    let mut nat = Nat::default();
    let inc = nat.inc;
    let mul = nat.mul;
    let one = nat.one;
    let square = nat.square;
    execute! {nat;
        (add e1 (one))
        (add e2 (inc e1))
        (add e4 (square e2))
        (add e16 (square e4))
    }
    while nat.run_ruleset("") {}

    index_dag!(Index
        expr: EExpr (datatype Expr
            One ()
            Inc (Expr)
            Mul (Expr Expr)
        )
    );

    let e4 = nat.get_val(4);
    let e13 = nat.get_val(13);
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
        let index = Index::extract(&nat, e, (one, inc, mul));

        let got = index.expr_to_string(e);
        assert!(expected.contains(&got.as_str()), "{expected:#?}    {got}");
    }

    impl Index {
        fn expr_to_string(&self, expr: TokenOpaque<Expr>) -> String {
            if !self.expr.contains_key(&expr) {
                return "???".to_string();
            }

            match self.expr[&expr] {
                EExpr::One() => format!("(one)"),
                EExpr::Inc(a) => format!("(inc {})", self.expr_to_string(a)),
                EExpr::Mul(a, b) => {
                    let mut a = self.expr_to_string(a);
                    let mut b = self.expr_to_string(b);
                    // To simplify testing
                    if a > b {
                        std::mem::swap(&mut a, &mut b);
                    }
                    format!("(mul {a} {b})")
                }
            }
        }
    }
}

#[test]
fn dag_basics_vec() {
    let mut nat = Nat::default();
    let inc = nat.inc;
    let one = nat.one;
    let square = nat.square;
    let expr_v = nat.expr_v;
    execute! {nat;
        (add e1 (one))
        (add e2 (inc e1))
        (add e4 (square e2))
        (add e16 (square e4))
    }
    while nat.run_ruleset("") {}

    index_dag!(Index
        expr: EExpr (datatype Expr
            ExprAny (String Expressions) {
                |(name, _), eg| ["one", "inc", "mul"].contains(&name.get(eg).as_str()).then(|| 1)
            }
        )
        [Expressions]
    );

    let e4 = nat.get_val(4);
    let e13 = nat.get_val(13);
    let e16 = nat.get_val(16);

    for (e, expected) in [
        (e4, &["(mul (inc (one)) (inc (one)))"][..]),
        (
            e13,
            &[
                "(inc (mul (inc (inc (inc (one)))) (inc (inc (one)))))",
                "(inc (mul (inc (one)) (mul (inc (inc (one))) (inc (one)))))",
                "(inc (mul (inc (inc (one))) (mul (inc (one)) (inc (one)))))",
            ],
        ),
        (
            e16,
            &["(mul (mul (inc (one)) (inc (one))) (mul (inc (one)) (inc (one))))"],
        ),
    ] {
        nat.for_each_row(expr_v, |(name, args), output| {
            eprintln!("{output:?} <- {} {:?}", name.get(&nat), args.get(&nat).0);
        });

        let index = Index::extract(&nat, e, expr_v);
        let got = index.expr_to_string(&nat, e);
        for (k, v) in index.expr.iter() {
            eprintln!("{k:?} -> {} {:?}", v.0.get(&nat), v.1.get(&nat).0);
        }
        assert!(expected.contains(&got.as_str()), "{expected:#?}    {got}");
    }

    impl Index {
        fn expr_to_string(&self, nat: &EGraph, expr: TokenOpaque<Expr>) -> String {
            if !self.expr.contains_key(&expr) {
                dbg!(expr);
                return "???".to_string();
            }

            let EExpr(op, args) = &self.expr[&expr];
            let ts = |arg: &_| self.expr_to_string(nat, *arg);
            let mut args: Vec<_> = args.get(nat).0.iter().map(ts).collect();
            args.sort();
            let mut r = format!("({}", op.get(nat));
            for arg in args {
                r.push(' ');
                r.push_str(&arg);
            }
            r.push(')');
            r
        }
    }
}
