use ahash::AHashSet;
use dateg::{ContainerVec, execute, rule, theory};

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

    (evaluation eval_add (usize usize) usize { |(a, b)| a + b })
)(
    (set_ruleset_active "eval")
    (rewrite (table_add (table_const a) (table_const b)) (table_const (eval_add b a)))
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
            (uni r (table_const 0))
        )

        (run_ruleset_active)
    }

    assert!(vaa.canon(&eg) == c0.canon(&eg));
}

#[test]
fn rewrite_helpers() {
    let Arithmetic {
        mut eg,
        table_sub,
        table_add,
        table_var,
        table_const,
        sa,
        sb,
        ..
    } = Arithmetic::default();

    execute! {eg;
        (add a (table_var sa))
        (add b (table_var sb))

        // ruleset by default: ""
        (rewrite (table_sub x x) (table_const 0))

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

#[test]
fn evaluation() {
    let mut ar = Arithmetic::default();
    let table_const = ar.table_const;
    let table_add = ar.table_add;
    let eval_add = ar.eval_add;
    let v0 = ar.v0;
    let v1 = ar.v1;
    let v5 = ar.v5;
    execute! {ar;
        (add c0 (table_const v0))
        (add c1 (table_const v1))
        (add c5 (table_const v5))

        (add c2 (table_add c1 c1))
        (add c3 (table_add c2 c1))
        (add c4 (table_add c3 c1))
        (add c5_ (table_add c4 c1))
    }

    assert!(c5.canon(&ar) != c5_.canon(&ar));
    while ar.run_ruleset("eval") {}
    assert!(c5.canon(&ar) == c5_.canon(&ar));

    execute! {ar;
        (add c6 (table_add c5 c1))
        // Adds c6 to table_const
        (run_ruleset "eval")

        (evaluation eval_mul (usize usize) usize { |(a, b)| a * b })
        (evaluation_partial not_is_one (usize) () { |(a,)| (a != 1).then(|| ()) })
        (evaluation print (usize usize usize) () { |(ab, a, b,)| println!("{ab} = {a} * {b}") })

        (relation is_divisible (Expr))
        (relation is_divisor (Expr))

        (set_ruleset_active "populate_divisible")
    }
    rule!(ar;
        // Function call args have to be bound
        (query _ (table_const a))
        (query _ (table_const b))
        // This is necessary to avoid generation of new values
        (query _ (table_const ab))

        // Function result can be unbound
        (query aa (eval_add a 0))

        // The main part of query lhs: `\exists a,b,ab \in universe: ab = a * b`
        (query ab (eval_mul a b))

        // Ensure we get natural divisors
        (contains (not_is_one a))
        (contains (not_is_one b))

        // Mark `ab` as divisible
        (insert (is_divisible (table_const ab)))

        // Demonstration of `call` usage
        (call (print ab a b))
        // Note, that calling `print` in `query` is also possible.
        // Code: `(query _r (print ab a b))`
    );
    assert!(ar.row_get(is_divisible, (c6,)).is_none());
    while ar.run_ruleset_active() {}
    assert!(ar.row_get(is_divisible, (c6,)).is_some());

    // panic!("GOOD")
}

#[test]
fn merge() {
    theory!(Geography(
        (sort Vertex)
        (ty usize)
    )(
        (constructor v (usize) Vertex)
        (function edge (Vertex Vertex) usize)
        (evaluation add (usize usize) usize { |(a, b)| a + b })
        (evaluation min (usize usize) usize { |(a, b)| {
            println!("min {a} {b}: {}", std::cmp::min(a, b));
            std::cmp::min(a, b)
        } })
        (function path (Vertex Vertex) usize :merge min)
    )(
        (rewrite (edge a b) (path a b))
    ));
    let mut ps = Geography::default();
    let v = ps.v;
    let edge = ps.edge;
    let path = ps.path;
    let add = ps.add;

    execute! {ps;
        (val c0 (usize) {0})
        (val c1 (usize) {1})
        (val c2 (usize) {2})
        (val c3 (usize) {3})
        (val c4 (usize) {4})
        (add v0 (v c0))
        (add v1 (v c1))
        (add v2 (v c2))
        (add v3 (v c3))
        (add v4 (v c4))

        (evaluation log_path_concat (Vertex Vertex Vertex usize) () { |(v1, v2, v3, len)| {
            if false {
                eprintln!("construct path {v1:?}->{v2:?}->{v3:?} of len {len}")
            }
        } })
    }

    rule!(ps;
        (query len (add (path a b) (path b c)))
        (set len (path a c))
        (call (log_path_concat a b c len))
    );
    rule!(ps;
        (query 0 (path a b))
        (uni a b)
    );

    execute! {ps;
        (set c1 (edge v0 v1))
        (set c1 (edge v1 v2))
        (set c1 (edge v2 v3))
        (set c1 (edge v1 v4))
    }
    while ps.run_ruleset_active() {}

    assert_eq!(ps.row_get(path, (v0, v3)).unwrap().get(&ps), 3);
    assert_eq!(ps.row_get(path, (v0, v4)).unwrap().get(&ps), 2);
    assert!(ps.row_get(path, (v2, v4)).is_none());

    execute! {ps;
        (set c0 (edge v3 v4))
    }
    while ps.run_ruleset_active() {}
    let v4 = v4.canon(&ps);
    assert_eq!(ps.row_get(path, (v2, v4)).unwrap().get(&ps), 1);
}

#[test]
fn container() {
    type CU = ContainerVec<Int>;
    theory!(TC(
        (ty usize)
        (sort Int)
        (container CU)
    )()());

    let mut tc = TC::default();

    execute! {tc;
        (constructor x () Int)
        (constructor y () Int)
        (constructor v (CU) Int)

        (evaluation_partial ensure_len_leq (CU usize) () { |(c, b)| (c.0.len() <= b).then(|| ()) })
        (evaluation_partial ensure_non_empty (CU) () { |(c,)| (!c.0.is_empty()).then(|| ()) })
        (evaluation_partial get (CU usize) Int { |(c, v)| c.0.get(v).copied() })
        (evaluation push (CU Int) CU { |(mut c, v)| { c.0.push(v); c } })
    }

    execute! {tc;
        (rule
            (query val (v vec))
            (query _ (ensure_len_leq vec 2))
            (query _ (ensure_non_empty vec))
            (query first (get vec 0))
            (uni val (v (push vec first)))
        )
    }

    let x = tc.row_add(x, ());
    let y = tc.row_add(y, ());

    let c_ = tc.add_container_value(ContainerVec::<Int>(vec![]));
    let c_x = tc.add_container_value(ContainerVec::<Int>(vec![x]));
    let c_x_y_x = tc.add_container_value(ContainerVec::<Int>(vec![x, y, x]));
    let c_x_x_x = tc.add_container_value(ContainerVec::<Int>(vec![x, x, x]));

    let v_ = tc.row_add(v, (c_,));
    let v_x = tc.row_add(v, (c_x,));
    let v_x_x_x = tc.row_add(v, (c_x_x_x,));
    let v_x_y_x = tc.row_add(v, (c_x_y_x,));

    assert!(v_x.canon(&tc) != v_x_x_x.canon(&tc));
    while tc.run_ruleset_active() {}
    assert!(v_x.canon(&tc) == v_x_x_x.canon(&tc));

    assert!(v_.canon(&tc) != v_x.canon(&tc));
    assert!(v_x.canon(&tc) != v_x_y_x.canon(&tc));

    execute! {tc; (rule (uni {x} {y})) }
    while tc.run_ruleset_active() {}

    assert!(v_.canon(&tc) != v_x.canon(&tc));
    assert!(v_x_x_x.canon(&tc) == v_x_y_x.canon(&tc));
}

#[test]
fn theory_inheritance() {
    theory!(Arithmetic2: Arithmetic {table_add};
        ()
        (
            (constructor table_add_v2 (Expr Expr) Expr)
        )
        (
            (rewrite (table_add a b) (table_add_v2 a b))
        )
    );
    let a = Arithmetic2::default();
    let _ = a.table_add;
    let _ = a.table_add_v2;
}
