#![allow(
    clippy::enum_variant_names,
    clippy::needless_range_loop,
    clippy::useless_format
)]

mod nat {
    #![allow(unused)]

    use dateg::{ContainerVec, EGraph, Token, TokenOpaque, TokenPrimitive, execute, theory};

    pub type Expressions = ContainerVec<Expr>;
    theory!(Nat(
        (ty String)
        (sort Expr)
        (sort ExprTuple)
        (container Expressions)
    )(
        (constructor inc (Expr) Expr)
        (constructor mul (Expr Expr) Expr)
        (constructor one () Expr)
        (constructor add (Expr Expr) Expr)
        (constructor square (Expr) Expr)

        (constructor expr (String ExprTuple) Expr)
        (constructor expr0 () ExprTuple)
        (constructor expr1 (Expr) ExprTuple)
        (constructor expr2 (Expr Expr) ExprTuple)
        (constructor expr_v (String Expressions) Expr)
        (evaluation expr0_v () Expressions { |()| ContainerVec(vec![]) })
        (evaluation expr1_v (Expr) Expressions { |(a,)| ContainerVec(vec![a]) })
        (evaluation expr2_v (Expr Expr) Expressions { |(a, b)| ContainerVec(vec![a, b]) })

        (relation is_comas (String))

        (val s_mul (String) {"mul".into()})
        (val s_add (String) {"add".into()})
    )(
        (insert (is_comas s_mul))
        (insert (is_comas s_add))

        (birewrite (one)        (expr "one" (expr0)))
        (birewrite (inc x)      (expr "inc" (expr1 x)))
        (birewrite (square x)   (expr "square" (expr1 x)))
        (birewrite (mul x y)    (expr "mul" (expr2 x y)))
        (birewrite (add x y)    (expr "add" (expr2 x y)))
        (rewrite (expr s (expr0)) (expr_v s (expr0_v)))
        (rewrite (expr s (expr1 a)) (expr_v s (expr1_v a)))
        (rewrite (expr s (expr2 a b)) (expr_v s (expr2_v a b)))

        (rewrite
            (expr name (expr2 x y))
            (expr name (expr2 y x))
            if (contains (is_comas name))
        )
        (birewrite
            (expr name (expr2 x (expr name (expr2 y z))))
            (expr name (expr2 (expr name (expr2 x y)) z))
            if (contains (is_comas name))
        )

        (birewrite (inc x) (add x (one)))
        (rule
            (query r (expr name args))
            (set r (mul r (one)))
        )
        (birewrite (add (mul x y) y) (mul (inc x) y))
        (birewrite (mul x x) (square x))
    ));

    impl Nat {
        pub fn get_val(&self, val: usize) -> TokenOpaque<Expr> {
            let one = self.one;
            let inc = self.inc;
            let mut expr = self.eg.row_get(one, ()).unwrap();
            for v in 1..val {
                let err = || panic!("{v}");
                expr = self.eg.row_get(inc, (expr,)).unwrap_or_else(err);
            }
            expr.canon(&self.eg)
        }
    }
}

use dateg::{EGraph, TokenOpaque, execute};

use dateg_extractors::tree::{CostFor, IndexFor, index_tree};
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

    index_tree!(Index
        expr: EExpr (datatype Expr
            One ()
            Inc (Expr)
            Mul (Expr Expr)
        )
    );

    let index = Index::extract(&nat, (one, inc, mul));

    fn eval(nat: &EGraph, index: &Index, expr: TokenOpaque<Expr>) -> usize {
        match index.value(expr.canon(nat)) {
            EExpr::One() => 1,
            EExpr::Inc(e) => eval(nat, index, e) + 1,
            EExpr::Mul(a, b) => eval(nat, index, a) * eval(nat, index, b),
        }
    }
    fn expr_to_strings(nat: &EGraph, index: &Index, expr: TokenOpaque<Expr>) -> Vec<String> {
        let mut r = vec![];
        for option in index.get_full(expr.canon(nat)).unwrap().1.iter() {
            match option {
                EExpr::One() => r.push(format!("o")),
                EExpr::Inc(e) => {
                    for e in expr_to_strings(nat, index, *e) {
                        r.push(format!("i{e}"));
                    }
                }
                EExpr::Mul(a, b) => {
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

    index_tree!(IndexFullC1
        expr: IFExprC1 (datatype Expr
            IFC1One ()
            IFC1Inc (Expr)
            IFC1Mul (Expr Expr)
            IFC1Square (Expr)
        )
    );
    let index_full_c1 = IndexFullC1::extract(&nat, (one, inc, mul, square));
    index_tree!(IndexFullC2
        expr: IFExprC2 (datatype Expr
            IFC2One ()
            IFC2Inc (Expr)
            IFC2Mul (Expr Expr)
            IFC2Square (Expr) {|(arg,), index, eg| Some(2 + index.cost(arg, eg)?) }
        )
    );
    let index_full_c2 = IndexFullC2::extract(&nat, (one, inc, mul, square));
    index_tree!(IndexFullC3
        expr: IFC3Expr (datatype Expr
            IFC3One ()
            IFC3Inc (Expr)
            IFC3Mul (Expr Expr)
            IFC3Square (Expr) {|(arg,), index, eg| Some(3 + index.cost(arg, eg)?) }
        )
    );
    let index_full_c3 = IndexFullC3::extract(&nat, (one, inc, mul, square));

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

    index_tree!(Index
        expr: EExpr (datatype Expr
            ExprAny (String ExprTuple)
        )
        expr_tuple: EExprTuple (datatype ExprTuple
            Expr0 ()
            Expr1 (Expr)
            Expr2 (Expr Expr)
        )
    );

    let index = Index::extract(&nat, expr, (expr0, expr1, expr2));

    impl Index {
        fn expr_to_string(&self, nat: &EGraph, expr: TokenOpaque<Expr>) -> String {
            let EExpr(op, args) = self.value(expr);
            match self.value(args) {
                EExprTuple::Expr0() => format!("({})", op.get(nat)),
                EExprTuple::Expr1(a) => {
                    format!("({} {})", op.get(nat), self.expr_to_string(nat, a))
                }
                EExprTuple::Expr2(a, b) => {
                    format!(
                        "({} {} {})",
                        op.get(nat),
                        self.expr_to_string(nat, a),
                        self.expr_to_string(nat, b),
                    )
                }
            }
        }
    }

    let e15 = nat.get_val(15);
    let r = index.expr_to_string(&nat, e15);
    assert_eq!(r, "(mul (inc (square (inc (one)))) (inc (inc (one))))");
}

#[test]
fn generic_vec() {
    let mut nat = Nat::default();
    let inc = nat.inc;
    let mul = nat.mul;
    let one = nat.one;
    let expr_v = nat.expr_v;
    execute! {nat;
        (add e1 (one))
        (add e2 (inc e1))
        (add e4 (mul e2 e2))
        (add e16 (mul e4 e4))
    }
    while nat.run_ruleset("") {}

    index_tree!(Index
        expr: EExpr (datatype Expr
            ExprAny (String Expressions)
        )
        [Expressions] // Unnecessary
    );

    let index = Index::extract(&nat, expr_v);

    impl Index {
        fn expr_to_string(&self, nat: &EGraph, expr: TokenOpaque<Expr>) -> String {
            let EExpr(op, args) = self.value(expr);
            let v = args.get(nat);
            match &v.0[..] {
                [] => format!("({})", op.get(nat)),
                &[a] => format!("({} {})", op.get(nat), self.expr_to_string(nat, a)),
                &[a, b] => format!(
                    "({} {} {})",
                    op.get(nat),
                    self.expr_to_string(nat, a),
                    self.expr_to_string(nat, b),
                ),
                [..] => panic!("{}", v.0.len()),
            }
        }
    }

    let e15 = nat.get_val(15);
    let r = index.expr_to_string(&nat, e15);
    assert_eq!(r, "(mul (inc (square (inc (one)))) (inc (inc (one))))");
}
