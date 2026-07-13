mod nat;

use dateg_extractors::define_index;
use dateg::{EGraph, TokenOpaque, execute};

pub struct Expr;
pub type TokenExpr = TokenOpaque<Expr>;

#[test]
fn dag_basic() {
    let mut eg = EGraph::default();
    execute! {eg;
        (constructor c1 () TokenExpr)
        (constructor c2 () TokenExpr)
        (constructor f (TokenExpr) TokenExpr)
        (constructor g (TokenExpr) TokenExpr)

        (= e1 (c1))
        (= e2 (f e1))
        (= e3 (f e2))
        (= e4 (f e3))
        (= e5 (f e4))
        (= e6 (f e5))
        (= x (g e6))

        (rule (uni {x} {e5}))
    }
    while eg.run_ruleset("") {}

    define_index!(Index
        (datatype TokenExpr -> Expr
            C1 ()
            F (TokenExpr)
            G (TokenExpr)
        )
    );
    let mut extractor = Index::extractor_dag_basic(&eg, (c1, f, g));

    let x = x.canon(&eg);
    let index: Index = extractor.extract(x).unwrap();
    impl Index {
        fn expr_to_string(&self, expr: TokenExpr) -> String {
            match self.expr[&expr].1[0] {
                Expr::C1(_) => format!("c1"),
                Expr::F((a,)) => format!("f {}", self.expr_to_string(a)),
                Expr::G((a,)) => format!("g {}", self.expr_to_string(a)),
            }
        }
    }

    assert_eq!(index.expr_to_string(x), "(f (f (f (f (c1)))))");
}
