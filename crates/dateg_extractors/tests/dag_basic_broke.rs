mod nat;

use dateg::{EGraph, EGraphValue, TokenOpaque, execute};
use dateg_extractors::define_index;

pub struct Expr;
impl EGraphValue for Expr {
    type Token = TokenExpr;
}
pub type TokenExpr = TokenOpaque<Expr>;

#[test]
#[ignore = "failing"]
fn dag_basic() {
    let mut eg = EGraph::default();
    execute! {eg;
        (constructor c1 () Expr)
        (constructor c2 () Expr)
        (constructor f (Expr) Expr)
        (constructor g (Expr) Expr)

        (add e1 (c1))
        (add e2 (f e1))
        (add e3 (f e2))
        (add e4 (f e3))
        (add e5 (f e4))
        (add e6 (f e5))
        (add x (g e6))

        (rule (uni {x} {e5}))
    }
    while eg.run_ruleset("") {}

    define_index!(Index
        (datatype Expr -> EExpr
            C1 ()
            F (Expr)
            G (Expr)
        )
    );
    let mut extractor = Index::extractor_dag_basic(&eg, (c1, f, g));

    let x = x.canon(&eg);
    let index: Index = extractor.extract(x).unwrap();
    impl Index {
        fn expr_to_string(&self, expr: TokenExpr) -> String {
            match self.e_expr[&expr].1[0] {
                EExpr::C1(_) => format!("c1"),
                EExpr::F((a,)) => format!("f {}", self.expr_to_string(a)),
                EExpr::G((a,)) => format!("g {}", self.expr_to_string(a)),
            }
        }
    }

    assert_eq!(index.expr_to_string(x), "(f (f (f (f (c1)))))");
}
