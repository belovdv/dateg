mod nat;

use dateg::{EGraph, EGraphValue, TokenOpaque, execute};
use dateg_extractors_macro::index_dag;

#[test]
fn dag_basic() {
    pub struct Expr;
    impl EGraphValue for Expr {
        type Token = TokenOpaque<Expr>;
    }

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

    index_dag!(Index
        expr: EExpr (datatype Expr
            C1 ()
            F (Expr)
            G (Expr)
        )
    );
    let x = x.canon(&eg);
    let index = Index::extract(&eg, x, (c1, f, g));
    impl Index {
        fn expr_to_string(&self, expr: TokenOpaque<Expr>) -> String {
            match self.expr[&expr] {
                EExpr::C1() => format!("(c1)"),
                EExpr::F(a) => format!("(f {})", self.expr_to_string(a)),
                EExpr::G(a) => format!("(g {})", self.expr_to_string(a)),
            }
        }
    }

    assert_eq!(index.expr_to_string(x), "(f (f (f (f (c1)))))");
}
