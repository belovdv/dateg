mod nat;

use std::collections::BTreeSet;

use ahash::AHashSet;
use dateg::{TokenOpaque, execute};

use dateg_extractors::dag::{Extractor, IndexFor, index_dag};
use du_utils_timed::timed_print;
use nat::*;

#[test]
fn dag_correctness() {
    let mut nat = Nat::default();
    let inc = nat.inc;
    let mul = nat.mul;
    let one = nat.one;
    let add = nat.add;
    let square = nat.square;
    execute! {nat;
        (add e1 (one))
        (add e2 (inc e1))
        (add e4 (square e2))
        (add e16 (square e4))
    }

    let tests = if std::env::var("DATEG_TEST_DAG_FULL").is_ok() {
        execute! {nat;
            (add e64 (mul e16 e4))
            (add e68 (add e64 e4))
            (add e70 (add e68 e2))
            (add e140 (mul e70 e2))
        }
        (1..=140).collect()
    } else {
        vec![1, 2, 3, 4, 5, 7, 9, 14, 15]
    };

    while nat.run_ruleset("") {}

    let max = *tests.last().unwrap();
    let costs = compute_costs(max, 9);
    assert!(costs.iter().skip(1).all(|&v| v != usize::MAX));

    for n in tests {
        let e = nat.get_val(n);
        let mut extractor = Extractor::<Index>::default();
        extractor.set_constructor::<One, _>(one);
        extractor.set_constructor::<Inc, _>(inc);
        extractor.set_constructor::<Mul, _>(mul);
        let index = extractor.extract(&nat, e);
        assert_eq!(index.eval(e), n);
        assert_eq!(index.collect_cost(e), costs[n]);
    }
}

#[test]
fn dag_many_loops() {
    let mut nat = Nat::default();
    let inc = nat.inc;
    let mul = nat.mul;
    let one = nat.one;
    let add = nat.add;
    let square = nat.square;
    execute! {nat;
        (constructor dec (Expr) Expr)
        (constructor sub (Expr Expr) Expr)

        // Introduce loops: e = (dec (inc e))
        (rule
            (query r (inc e))
            (set e (dec r))
        )
        (rule
            (query r (add a b))
            (set a (sub r b))
        )

        (add e1 (one))
        (add e2 (inc e1))
        (add e4 (square e2))
        (add e16 (square e4))
    }

    let costs = vec![
        0, 1, 2, 3, 3, 4, 4, 5, 4, 4, 5, 6, 5, 6, 5, 5, 4, 5, 5, 6, 5, 6, 7, 6, 6, 5, 6, 5, 6, 7,
        6, 6, 5,
    ];
    // Important: 14, 15, 23, 31
    // Slow:
    let mut tests: Vec<_> = (1..=31).collect();
    if std::env::var("DATEG_TEST_DAG_FULL").is_ok() {
        execute! {nat;
            (add e32 (mul e16 e2))
            (add e48 (add e32 e16))
        }
    } else {
        execute! {nat;
            (add e20 (add e16 e4))
        }
        tests.retain(|n| *n <= 17 && ![7, 11, 13].contains(n));
    }

    env_logger::init();

    timed_print("nat.run_ruleset", 100, || while nat.run_ruleset("") {});

    use rayon::prelude::*;
    tests.into_par_iter().for_each(|n| {
        let cost = costs[n];
        let e = nat.get_val(n);
        let index = timed_print(format!("Index::extract {n}"), 100, || {
            Index::extract(&nat, e, (one, inc, mul, sub))
        });
        assert_eq!(index.eval(e), n);
        assert_eq!(index.collect_cost(e), cost, "{n}");
    });
}

index_dag!(Index
    expr: EExpr (datatype Expr
        One ()
        Inc (Expr)
        Mul (Expr Expr)
        Sub (Expr Expr)
    )
);

impl Index {
    fn collect_cost(&self, expr: TokenOpaque<Expr>) -> usize {
        let mut r = Default::default();
        self.collect_cost_(expr, &mut r);
        r.len()
    }
    fn collect_cost_(&self, expr: TokenOpaque<Expr>, set: &mut AHashSet<TokenOpaque<Expr>>) {
        if set.insert(expr) {
            match self.value(expr) {
                EExpr::One() => {}
                EExpr::Inc(a) => self.collect_cost_(a, set),
                EExpr::Mul(a, b) | EExpr::Sub(a, b) => {
                    self.collect_cost_(a, set);
                    self.collect_cost_(b, set);
                }
            }
        }
    }

    fn eval(&self, expr: TokenOpaque<Expr>) -> usize {
        match self.value(expr) {
            EExpr::One() => 1,
            EExpr::Inc(v) => self.eval(v) + 1,
            EExpr::Mul(a, b) => self.eval(a) * self.eval(b),
            EExpr::Sub(a, b) => self.eval(a) - self.eval(b),
        }
    }
}

fn compute_costs(max_value: usize, max_cost: usize) -> Vec<usize> {
    let mut costs = vec![usize::MAX; max_value + 1];
    costs[1] = 1;
    // At length 1 we have a single available set {1}
    // Note: we won't keep sets that were available on previous steps (duplicated values)
    let mut available: AHashSet<BTreeSet<usize>> =
        [[1].into_iter().collect()].into_iter().collect();

    for len in 2..=max_cost {
        let mut next = AHashSet::default();
        for available in available {
            let mut values = AHashSet::new();
            for a in available.iter().copied() {
                values.insert(a + 1);
                for b in available.iter().copied() {
                    values.insert(a * b);
                }
            }
            for value in values {
                if value <= max_value && !available.contains(&value) {
                    let mut new = available.clone();
                    new.insert(value);
                    next.insert(new);
                    costs[value] = std::cmp::min(costs[value], len);
                }
            }
        }
        available = next.into_iter().collect();
    }
    costs
}
