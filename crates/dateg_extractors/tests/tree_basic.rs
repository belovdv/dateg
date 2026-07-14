mod nat;

use dateg::{EGraph, TokenOpaque, execute};
use dateg_extractors::{IndexFor, define_index};

use nat::*;

#[test]
fn concrete() {
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

    define_index!(Index
        (datatype Expr -> EExpr
            One ()
            Inc (Expr)
            Mul (Expr Expr)
        )
    );
    let index = Index::extractor_tree_basic(&nat, (one, inc, mul));

    fn eval(nat: &EGraph, index: &Index, expr: TokenOpaque<Expr>) -> usize {
        match index.value(expr.canon(nat)) {
            EExpr::One(()) => 1,
            EExpr::Inc((e,)) => eval(nat, index, e) + 1,
            EExpr::Mul((a, b)) => eval(nat, index, a) * eval(nat, index, b),
        }
    }
    fn expr_to_strings(nat: &EGraph, index: &Index, expr: TokenOpaque<Expr>) -> Vec<String> {
        let mut r = vec![];
        for option in index.get_full(expr.canon(nat)).unwrap().1.iter() {
            match option {
                EExpr::One(()) => r.push(format!("o")),
                EExpr::Inc((e,)) => {
                    for e in expr_to_strings(nat, index, *e) {
                        r.push(format!("i{e}"));
                    }
                }
                EExpr::Mul((a, b)) => {
                    for a in expr_to_strings(nat, index, *a) {
                        for b in expr_to_strings(nat, index, *b) {
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
    let mut expr = nat.row_get(one, ()).unwrap();
    for value in 1..=13 {
        assert_eq!(value, eval(&nat, &index, expr));
        let (cost, _) = index.get_full(expr.canon(&nat)).unwrap();
        let mut options = expr_to_strings(&nat, &index, expr);
        for option in options.iter() {
            assert_eq!(option.len(), *cost);
        }
        options.sort();
        let options: Vec<_> = options.iter().map(|s| s.as_str()).collect();
        assert_eq!(options, results[value]);
        expr = nat.row_get(inc, (expr,)).unwrap();
    }

    define_index!(IndexFullC1
        (datatype Expr -> IFExprC1
            IFC1One ()
            IFC1Inc (Expr)
            IFC1Mul (Expr Expr)
            IFC1Square (Expr)
        )
    );
    let index_full_c1 = IndexFullC1::extractor_tree_basic(&nat, (one, inc, mul, square));
    define_index!(IndexFullC2
        (datatype Expr -> IFExprC2
            IFC2One ()
            IFC2Inc (Expr)
            IFC2Mul (Expr Expr)
            IFC2Square (Expr) :cost 2
        )
    );
    let index_full_c2 = IndexFullC2::extractor_tree_basic(&nat, (one, inc, mul, square));
    define_index!(IndexFullC3
        (datatype Expr -> IFC3Expr
            IFC3One ()
            IFC3Inc (Expr)
            IFC3Mul (Expr Expr)
            IFC3Square (Expr) :cost 3
        )
    );
    let index_full_c3 = IndexFullC3::extractor_tree_basic(&nat, (one, inc, mul, square));

    let e4 = nat.get_val(4);
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
    let mut nat = Nat::default();
    let inc = nat.inc;
    let mul = nat.mul;
    let one = nat.one;
    let expr = nat.expr;
    let expr0 = nat.expr0;
    let expr1 = nat.expr1;
    let expr2 = nat.expr2;
    execute! {nat;
        (add e1 (one))
        (add e2 (inc e1))
        (add e4 (mul e2 e2))
        (add e16 (mul e4 e4))
    }
    while nat.run_ruleset("") {}

    define_index!(Index
        (datatype Expr -> EExpr
            ExprAny (String ExprTuple)
        )
        (datatype ExprTuple -> EExprTuple
            Expr0 ()
            Expr1 (Expr)
            Expr2 (Expr Expr)
        )
    );

    let index = Index::extractor_tree_basic(&nat, expr, (expr0, expr1, expr2));

    impl Index {
        fn expr_to_string(&self, nat: &EGraph, expr: TokenOpaque<Expr>) -> String {
            match self.value(expr) {
                EExpr::ExprAny((op, args)) => match self.value(args) {
                    EExprTuple::Expr0(()) => format!("({})", op.get(nat)),
                    EExprTuple::Expr1((a,)) => {
                        format!("({} {})", op.get(nat), self.expr_to_string(nat, a))
                    }
                    EExprTuple::Expr2((a, b)) => {
                        format!(
                            "({} {} {})",
                            op.get(nat),
                            self.expr_to_string(nat, a),
                            self.expr_to_string(nat, b),
                        )
                    }
                },
            }
        }
    }

    let e15 = nat.get_val(15);
    let r = index.expr_to_string(&nat, e15);
    assert_eq!(r, "(mul (inc (square (inc (one)))) (inc (inc (one))))");
}
