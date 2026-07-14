use ahash::AHashSet;
use dateg::{EGraph, execute, rule, theory};

theory!(Arithmetic(
    (sort Expr)
    (ty usize)
    (ty String)
)(
    (constructor table_add (Expr Expr) Expr)
    (constructor table_sub (Expr Expr) Expr)
    (constructor table_mul (Expr Expr) Expr)
    (constructor table_const (usize) Expr)
    (constructor table_var (String) Expr)
    (function universe (String) Expr)

    (val v0 (usize) {0})
    (val v1 (usize) {1})
    (val v5 (usize) {5})
    (val sa (String) {"a".into()})
    (val sb (String) {"b".into()})
    (val sc (String) {"c".into()})
)(
));

#[test]
fn add_and_get_values() {
    let Arithmetic {
        mut eg,
        table_add,
        table_mul,
        table_const,
        table_var,
        v1,
        v5,
        sa,
        sb,
        ..
    } = Arithmetic::default();

    execute! {eg;
        (add c1 (table_const v1))
        (add _c5 (table_const v5))
        (add va (table_var sa))
        (add vb (table_var sb))

        (add e_add_a_b (table_add va vb))
        (add e_add_b_1 (table_add vb c1))
        (add e_add_a_add_b_1 (table_add va e_add_b_1))
        (add _e_mul_a_1 (table_mul va c1))
    }

    execute! {eg;
        (add e_sq_sum_a_b (table_mul e_add_a_b e_add_a_b))

        // Error: expected (..., ...), found (..., ..., ...)
        // (add e_mul (table_mul va c1 vb))
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
    let Arithmetic {
        mut eg,
        table_add,
        table_sub,
        table_var,
        sa,
        sb,
        sc,
        ..
    } = Arithmetic::default();
    execute! {eg;
        (add va (table_var sa))
        (add vb (table_var sb))
        (add vc (table_var sc))

        (add ab (table_add va vb))
        (add cb (table_sub vc vb))
        (add ab_cb (table_add ab cb)) // (a + b) + (c - b)
        (add ac (table_add va vc))    // (a + c)
    }
    assert!(ab_cb.canon(&eg) != ac.canon(&eg));

    // (x + y) + z -> x + (y + z)
    let r1 = rule!(eg;
        (query r (table_add (table_add x y) z))
        (set r (table_add x (table_add y z)))
    );

    // x + (y - x) -> y
    let r2 = rule!(eg;
        (query r (table_add x (table_sub y x)))
        (uni r y)
    );

    eg.run_rules(&[r2]);
    assert!(ab_cb.canon(&eg) != ac.canon(&eg));
    eg.run_rules(&[r1]);
    assert!(ab_cb.canon(&eg) != ac.canon(&eg));
    eg.run_rules(&[r2]);
    assert!(ab_cb.canon(&eg) == ac.canon(&eg));
}

#[test]
fn rule_builder_external_value() {
    let Arithmetic {
        mut eg,
        table_sub,
        table_var,
        table_const,
        v0,
        sa,
        ..
    } = Arithmetic::default();

    execute! {eg;
        (add c0 (table_const v0))
        (add va (table_var sa))
        (add vaa (table_sub va va))

        // x - x -> 0
        (rule
            (query r (table_sub x x))
            (uni r {c0})
        )

        (run_ruleset_active)
    }

    assert!(vaa.canon(&eg) == c0.canon(&eg));
}

#[test]
fn rewrite_helpers() {
    let mut eg = EGraph::default();

    eg.add_primitive_type::<String>();
    eg.add_primitive_type::<usize>();

    let v0 = eg.add_primitive_value(0usize);
    let sa = eg.add_primitive_value("a".to_string());
    let sb = eg.add_primitive_value("b".to_string());

    execute! {eg;
        (constructor table_add (Expr Expr) Expr)
        (constructor table_sub (Expr Expr) Expr)
        (constructor table_var (String) Expr)
        (constructor table_const (usize) Expr)

        (add c0 (table_const v0))
        (add a (table_var sa))
        (add b (table_var sb))

        // ruleset by default: ""
        (rewrite (table_sub x x) {c0})

        (set_ruleset_active "arithmetic")
        (rewrite (table_add x y) (table_add y x))
        (birewrite
            (table_add x (table_add y z))
            (table_add (table_add x y) z)
        )

        (add ab (table_add a b))
        (add ab_b (table_add ab b))
        (add b_ab (table_add b ab))

        (run_ruleset "")
    }

    assert!(ab_b.canon(&eg) != b_ab.canon(&eg));
    eg.run_ruleset_active();
    assert!(ab_b.canon(&eg) == b_ab.canon(&eg));
}
