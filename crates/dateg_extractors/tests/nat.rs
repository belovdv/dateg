#![allow(unused)]

use dateg::{EGraph, Token, TokenOpaque, TokenPrimitive, execute};

pub struct Expr;
pub struct ExprTuple;
pub type TokenExpr = TokenOpaque<Expr>;
pub type TokenExprTuple = TokenOpaque<ExprTuple>;
pub type TokenString = TokenPrimitive<String>;

pub fn eg() -> EGraph {
    let mut eg = EGraph::default();

    eg.add_primitive_type::<String>();
    eg.add_primitive_type::<()>();

    let s_one = eg.add_primitive_value("one".to_string());
    let s_inc = eg.add_primitive_value("inc".to_string());
    let s_mul = eg.add_primitive_value("mul".to_string());
    let s_add = eg.add_primitive_value("add".to_string());
    let s_square = eg.add_primitive_value("square".to_string());

    execute! {eg;
        (constructor inc (TokenExpr) TokenExpr)
        (constructor mul (TokenExpr TokenExpr) TokenExpr)
        (constructor one () TokenExpr)
        (constructor add (TokenExpr TokenExpr) TokenExpr)
        (constructor square (TokenExpr) TokenExpr)

        (constructor expr (TokenString TokenExprTuple) TokenExpr)
        (constructor expr0 () TokenExprTuple)
        (constructor expr1 (TokenExpr) TokenExprTuple)
        (constructor expr2 (TokenExpr TokenExpr) TokenExprTuple)

        (relation is_comas (TokenString))
        (= () (is_comas s_mul))
        (= () (is_comas s_add))


        (birewrite (one)        (expr {s_one} (expr0)))
        (birewrite (inc x)      (expr {s_inc} (expr1 x)))
        (birewrite (square x)   (expr {s_square} (expr1 x)))
        (birewrite (mul x y)    (expr {s_mul} (expr2 x y)))
        (birewrite (add x y)    (expr {s_add} (expr2 x y)))

        (rewrite
            (expr name (expr2 x y))
            (expr name (expr2 y x))
            if (query u (is_comas name))
        )
        (birewrite
            (expr name (expr2 x (expr name (expr2 y z))))
            (expr name (expr2 (expr name (expr2 x y)) z))
            if (query u (is_comas name))
        )

        (birewrite (inc x) (add x (one)))
        (rule
            (query r (expr name args))
            (set r (mul r (one)))
        )
        (birewrite (add (mul x y) y) (mul (inc x) y))
        (birewrite (mul x x) (square x))
    }

    eg
}

pub fn get_val(eg: &mut EGraph, val: usize) -> TokenExpr {
    execute! {eg;
        (get_constructor one () TokenExpr)
        (get_constructor inc (TokenExpr) TokenExpr)
    }
    let mut expr = eg.row_get(one, ()).unwrap();
    for _ in 1..val {
        expr = eg.row_get(inc, (expr,)).unwrap();
    }
    expr.canon(eg)
}
