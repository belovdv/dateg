mod nat;

use dateg::execute;
use dateg_extractors::define_index;

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

    define_index!(Index
        (datatype TokenExpr -> Expr
            One ()
            Inc (TokenExpr)
            Mul (TokenExpr TokenExpr)
        )
    );
    let mut extractor = Index::extractor_dag_basic(&eg, (one, inc, mul));

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
        let index: Index = extractor.extract(e).unwrap();

        let got = index.expr_to_string(e);
        assert!(expected.contains(&got.as_str()), "{expected:#?}    {got}");
    }

    impl Index {
        fn expr_to_string(&self, expr: TokenExpr) -> String {
            match self.expr[&expr].1[0] {
                Expr::One(()) => format!("(one)"),
                Expr::Inc((a,)) => format!("(inc {})", self.expr_to_string(a)),
                Expr::Mul((a, b)) => {
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
