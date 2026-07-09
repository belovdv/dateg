use dateg::{EGraph, TokenValueOpaque, TokenValuePrimitive, execute};

pub struct Expr;
pub struct ExprTuple;
pub type TokenExpr = TokenValueOpaque<Expr>;
pub type TokenExprTuple = TokenValueOpaque<ExprTuple>;
pub type TokenString = TokenValuePrimitive<String>;

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

        (rule
            (query r (expr name (expr2 x y)))
            (query u (is_comas name))
            (set r (expr name (expr2 y x)))
        )
        (rule
            (query r (expr name (expr2 x (expr name (expr2 y z)))))
            (query u (is_comas name))
            (set r (expr name (expr2 (expr name (expr2 x y)) z)))
        )
        (rule
            (query r (expr name (expr2 (expr name (expr2 x y)) z)))
            (query u (is_comas name))
            (set r (expr name (expr2 x (expr name (expr2 y z)))))
        )

        (birewrite (inc x) (add x (one)))
        (rule // x -> 1 * x
            (query r (expr name args))
            (set r (mul r (one)))
        )
        (birewrite (add (mul x y) y) (mul (inc x) y))
        (birewrite (mul x x) (square x))
    }

    eg
}
