use ahash::AHashSet;
use dateg::{EGraph, Token, TokenValueOpaque, TokenValuePrimitive, execute};

struct Expr;
type TokenExpr = TokenValueOpaque<Expr>;
type TokenUsize = TokenValuePrimitive<usize>;
type TokenString = TokenValuePrimitive<String>;

#[test]
fn add_and_get_values() {
    let mut eg = EGraph::default();

    eg.add_primitive_type::<usize>();
    eg.add_primitive_type::<String>();

    let v1 = eg.add_primitive_value::<usize>(1);
    let v5 = eg.add_primitive_value::<usize>(5);
    let sa = eg.add_primitive_value("a".to_string());
    let sb = eg.add_primitive_value("b".to_string());

    execute! {eg;
        (table_add = constructor Add (TokenExpr TokenExpr) TokenExpr)
        (table_mul = constructor Mul (TokenExpr TokenExpr) TokenExpr)
        (table_const = constructor Const (TokenUsize) TokenExpr)
        (table_var = constructor Const (TokenString) TokenExpr)
        (universe = function Universe (TokenString) TokenExpr)

        (c1 = (table_const v1))
        (c5 = (table_const v5))
        (va = (table_var sa))
        (vb = (table_var sb))

        (e_add_a_b = (table_add va vb))
        (e_add_b_1 = (table_add vb c1))
        (e_add_a_add_b_1 = (table_add va e_add_b_1))
        (e_mul_a_1 = (table_mul va c1))
    }

    execute! {eg;
        (e_sq_sum_a_b = (table_mul e_add_a_b e_add_a_b))

        // Error: expected (..., ...), found (..., ..., ...)
        // (e_mul = (table_mul va c1 vb))
    }

    let e_sq_sum_a_b_ = eg.add_value(table_mul, (e_add_a_b, e_add_a_b));
    assert!(e_sq_sum_a_b == e_sq_sum_a_b_.canon(&eg));

    let mut rows: AHashSet<_> = [
        (va, vb, e_add_a_b),
        (vb, c1, e_add_b_1),
        (va, e_add_b_1, e_add_a_add_b_1),
    ]
    .into_iter()
    .collect();
    eg.for_each_row(table_add, |row| {
        assert!(rows.remove(&row));
    });
    assert!(rows.is_empty());

    let mut values: AHashSet<_> = [1, 5].into_iter().collect();
    eg.for_each_row(table_const, |(val, _)| {
        let val = val.get(&eg);
        assert!(values.remove(&val));
    });
    assert!(values.is_empty());
}
