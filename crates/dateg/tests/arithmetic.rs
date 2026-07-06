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
        (constructor table_add (TokenExpr TokenExpr) TokenExpr)
        (constructor table_mul (TokenExpr TokenExpr) TokenExpr)
        (constructor table_const (TokenUsize) TokenExpr)
        (constructor table_var (TokenString) TokenExpr)
        (function _universe (TokenString) TokenExpr)

        (= c1 (table_const v1))
        (= _c5 (table_const v5))
        (= va (table_var sa))
        (= vb (table_var sb))

        (= e_add_a_b (table_add va vb))
        (= e_add_b_1 (table_add vb c1))
        (= e_add_a_add_b_1 (table_add va e_add_b_1))
        (= _e_mul_a_1 (table_mul va c1))
    }

    execute! {eg;
        (= e_sq_sum_a_b (table_mul e_add_a_b e_add_a_b))

        // Error: expected (..., ...), found (..., ..., ...)
        // (e_mul = (table_mul va c1 vb))
    }

    let e_sq_sum_a_b_ = eg.row_add(table_mul, (e_add_a_b, e_add_a_b));
    assert!(e_sq_sum_a_b == e_sq_sum_a_b_.canon(&eg));

    let mut rows: AHashSet<_> = [
        ((va, vb), e_add_a_b),
        ((vb, c1), e_add_b_1),
        ((va, e_add_b_1), e_add_a_add_b_1),
    ]
    .into_iter()
    .collect();
    eg.for_each_row(table_add, |inputs, output| {
        assert!(rows.remove(&(inputs, output)));
    });
    assert!(rows.is_empty());

    let mut values: AHashSet<_> = [1, 5].into_iter().collect();
    eg.for_each_row(table_const, |(val,), _| {
        let val = val.get(&eg);
        assert!(values.remove(&val));
    });
    assert!(values.is_empty());
}

#[test]
fn rule_builder() {
    let mut eg = EGraph::default();

    eg.add_primitive_type::<String>();

    let sa = eg.add_primitive_value("a".to_string());
    let sb = eg.add_primitive_value("b".to_string());
    let sc = eg.add_primitive_value("c".to_string());

    execute! {eg;
        (constructor table_add (TokenExpr TokenExpr) TokenExpr)
        (constructor table_sub (TokenExpr TokenExpr) TokenExpr)
        (constructor table_var (TokenString) TokenExpr)

        (= va (table_var sa))
        (= vb (table_var sb))
        (= vc (table_var sc))

        (= ab (table_add va vb))
        (= cb (table_sub vc vb))
        (= ab_cb (table_add ab cb)) // (a + b) + (c - b)
        (= ac (table_add va vc))    // (a + c)
    }
    assert!(ab_cb.canon(&eg) != ac.canon(&eg));

    execute! {eg;
        // (x + y) + z -> x + (y + z)
        (rule r1 { x y z r }
            (query r (table_add (table_add x y) z))
            (set r (table_add x (table_add y z)))
        )

        // x + (y - x) -> y
        (rule r2 { x y r }
            (query r (table_add x (table_sub y x)))
            (uni r y)
        )

        // check nested `add`
        (rule _syntax { x y r }
            (query r (table_add x y))
            (add _xy2 (table_add (table_add x y) (table_add x y)))
        )
    }

    eg.run_single_rule(&[r2]);
    assert!(ab_cb.canon(&eg) != ac.canon(&eg));
    eg.run_single_rule(&[r1]);
    assert!(ab_cb.canon(&eg) != ac.canon(&eg));
    eg.run_single_rule(&[r2]);
    assert!(ab_cb.canon(&eg) == ac.canon(&eg));
}
