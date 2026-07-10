mod nat;

use dateg::{EGraph, Token, execute};
use dateg_extractors::{IndexFor, define_index};

use crate::nat::{TokenExpr, TokenExprTuple, TokenString, get_val};

#[test]
fn concrete() {
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
    let index = Index::extractor_tree_basic(&eg, (one, inc, mul));

    fn eval(eg: &EGraph, index: &Index, expr: TokenExpr) -> usize {
        match index.value(expr.canon(eg)) {
            Expr::One(()) => 1,
            Expr::Inc((e,)) => eval(eg, index, e) + 1,
            Expr::Mul((a, b)) => eval(eg, index, a) * eval(eg, index, b),
        }
    }
    fn expr_to_strings(eg: &EGraph, index: &Index, expr: TokenExpr) -> Vec<String> {
        let mut r = vec![];
        for option in index.get_full(expr.canon(eg)).unwrap().1.iter() {
            match option {
                Expr::One(()) => r.push(format!("o")),
                Expr::Inc((e,)) => {
                    for e in expr_to_strings(eg, index, *e) {
                        r.push(format!("i{e}"));
                    }
                }
                Expr::Mul((a, b)) => {
                    for a in expr_to_strings(eg, index, *a) {
                        for b in expr_to_strings(eg, index, *b) {
                            r.push(format!("m{a}{b}"));
                        }
                    }
                }
            }
        }
        r
    }

    let results = [
        &[][..],
        &["o"],
        &["io"],
        &["iio"],
        &["iiio"],
        &["iiiio"],
        &["iiiiio", "miioio", "mioiio"],
        &["iiiiiio", "imiioio", "imioiio"],
        &["miiioio", "mioiiio"],
        &["miioiio"],
        &["imiioiio", "miiiioio", "mioiiiio"],
        &["iimiioiio", "imiiiioio", "imioiiiio"],
        &["miiioiio", "miioiiio"],
        &["imiiioiio", "imiioiiio"],
    ];
    let mut expr = eg.row_get(one, ()).unwrap();
    for value in 1..=13 {
        assert_eq!(value, eval(&eg, &index, expr));
        let (cost, _) = index.get_full(expr.canon(&eg)).unwrap();
        let mut options = expr_to_strings(&eg, &index, expr);
        for option in options.iter() {
            assert_eq!(option.len(), *cost);
        }
        options.sort();
        let options: Vec<_> = options.iter().map(|s| s.as_str()).collect();
        assert_eq!(options, results[value]);
        expr = eg.row_get(inc, (expr,)).unwrap();
    }

    define_index!(IndexFullC1
        (datatype TokenExpr -> IFExprC1
            IFC1One ()
            IFC1Inc (TokenExpr)
            IFC1Mul (TokenExpr TokenExpr)
            IFC1Square (TokenExpr)
        )
    );
    let index_full_c1 = IndexFullC1::extractor_tree_basic(&eg, (one, inc, mul, square));
    define_index!(IndexFullC2
        (datatype TokenExpr -> IFExprC2
            IFC2One ()
            IFC2Inc (TokenExpr)
            IFC2Mul (TokenExpr TokenExpr)
            IFC2Square (TokenExpr) :cost 2
        )
    );
    let index_full_c2 = IndexFullC2::extractor_tree_basic(&eg, (one, inc, mul, square));
    define_index!(IndexFullC3
        (datatype TokenExpr -> IFC3Expr
            IFC3One ()
            IFC3Inc (TokenExpr)
            IFC3Mul (TokenExpr TokenExpr)
            IFC3Square (TokenExpr) :cost 3
        )
    );
    let index_full_c3 = IndexFullC3::extractor_tree_basic(&eg, (one, inc, mul, square));

    let e4 = get_val(&mut eg, 4);
    // square is not used, there is no such thing
    assert_eq!(index.get_full(e4).unwrap().1.len(), 1);
    assert_eq!(index.get_full(e4).unwrap().0, 4);
    // only square is used: cost(iiio) > cost(sio)
    assert_eq!(index_full_c1.get_full(e4).unwrap().1.len(), 1);
    assert_eq!(index_full_c1.get_full(e4).unwrap().0, 3);
    // square is used: cost(iiio) = cost(sio)
    assert_eq!(index_full_c2.get_full(e4).unwrap().1.len(), 2);
    assert_eq!(index_full_c2.get_full(e4).unwrap().0, 4);
    // square is not used: cost(iiio) < cost(sio)
    assert_eq!(index_full_c3.get_full(e4).unwrap().1.len(), 1);
    assert_eq!(index_full_c3.get_full(e4).unwrap().0, 4);
}

#[test]
fn generic() {
    let mut eg = nat::eg();
    execute! {eg;
        (constructor expr (TokenString TokenExprTuple) TokenExpr)
        (constructor expr0 () TokenExprTuple)
        (constructor expr1 (TokenExpr) TokenExprTuple)
        (constructor expr2 (TokenExpr TokenExpr) TokenExprTuple)

        (constructor one () TokenExpr)
        (constructor inc (TokenExpr) TokenExpr)
        (constructor mul (TokenExpr TokenExpr) TokenExpr)

        (= e1 (one))
        (= e2 (inc e1))
        (= e4 (mul e2 e2))
        (= e16 (mul e4 e4))
    }
    while eg.run_ruleset("") {}

    define_index!(Index
        (datatype TokenExpr -> Expr
            ExprAny (TokenString TokenExprTuple)
        )
        (datatype TokenExprTuple -> ExprTuple
            Expr0 ()
            Expr1 (TokenExpr)
            Expr2 (TokenExpr TokenExpr)
        )
    );

    let index = Index::extractor_tree_basic(&eg, expr, (expr0, expr1, expr2));

    impl Index {
        fn expr_to_string(&self, eg: &EGraph, expr: TokenExpr) -> String {
            match self.value(expr) {
                Expr::ExprAny((op, args)) => match self.value(args) {
                    ExprTuple::Expr0(()) => format!("({})", op.get(eg)),
                    ExprTuple::Expr1((a,)) => {
                        format!("({} {})", op.get(eg), self.expr_to_string(eg, a))
                    }
                    ExprTuple::Expr2((a, b)) => {
                        format!(
                            "({} {} {})",
                            op.get(eg),
                            self.expr_to_string(eg, a),
                            self.expr_to_string(eg, b),
                        )
                    }
                },
            }
        }
    }

    let e15 = get_val(&mut eg, 15);
    let r = index.expr_to_string(&eg, e15);
    assert_eq!(r, "(mul (inc (square (inc (one)))) (inc (inc (one))))");
}
