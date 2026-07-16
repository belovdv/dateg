mod nat;

use dateg::{TokenOpaque, execute};

use dateg_extractors_macro::index_dag;
use nat::*;

#[test]
fn dag_basic() {
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
