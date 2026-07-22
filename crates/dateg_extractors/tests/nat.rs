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

    (val s_one (String) {"one".into()})
    (val s_inc (String) {"inc".into()})
    (val s_mul (String) {"mul".into()})
    (val s_add (String) {"add".into()})
    (val s_square (String) {"square".into()})
)(
    (insert (is_comas s_mul))
    (insert (is_comas s_add))

    (birewrite (one)        (expr {s_one} (expr0)))
    (birewrite (inc x)      (expr {s_inc} (expr1 x)))
    (birewrite (square x)   (expr {s_square} (expr1 x)))
    (birewrite (mul x y)    (expr {s_mul} (expr2 x y)))
    (birewrite (add x y)    (expr {s_add} (expr2 x y)))
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
