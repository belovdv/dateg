use dateg::{TokenOpaque, execute};
use dateg_extractors_testkit::algorithms::{self, Algorithm};
use dateg_extractors_testkit::indexes::expr::{extract_expr_string, extract_loop_string};
use dateg_extractors_testkit::indexes::mem::{MemIndex, linearize_mem};
use dateg_extractors_testkit::memory::{Int, Memory, inc_chain};
use dateg_extractors_testkit::sources::generated::{self, Params, memory_weights};
use rayon::ThreadPoolBuilder;

fn run_chain(len: usize, algorithm: &Algorithm) -> (usize, String) {
    let (m, root) = inc_chain(len);
    extract_expr_string(&m, root, algorithm).unwrap()
}

/// Negating a negated value in the same e-class creates cyclic candidates while
/// the acyclic `neg(neg(...))` path remains available.
fn loop_fixture(len: usize) -> (Memory, TokenOpaque<Int>) {
    let mut m = Memory::default();
    let one = m.add_one();
    let mut a = m.add_neg(one);
    for _ in 1..len {
        a = m.add_neg(a);
    }
    let b = m.add_neg(a);
    execute! {m;
        (rule (uni {a} {b}))
    }
    while m.run_ruleset("") {}
    let root = a.canon(&m.eg);
    (m, root)
}

fn run_loop(len: usize, algorithm: &Algorithm) -> (usize, String) {
    let (m, root) = loop_fixture(len);
    extract_loop_string(&m, root, algorithm).unwrap()
}

#[test]
fn determinism_canary() {
    let pool = ThreadPoolBuilder::new().num_threads(1).build().unwrap();
    pool.install(|| {
        for algorithm in algorithms::all() {
            for len in 3..=6 {
                let first = run_chain(len, algorithm);
                for _ in 0..10 {
                    assert_eq!(
                        first,
                        run_chain(len, algorithm),
                        "{} chain length {len}",
                        algorithm.name
                    );
                }
            }
            for len in 1..=4 {
                let first = run_loop(len, algorithm);
                for _ in 0..10 {
                    assert_eq!(
                        first,
                        run_loop(len, algorithm),
                        "{} loop length {len}",
                        algorithm.name
                    );
                }
            }
        }
    });
}

const ORDER_A: &[&str] = &["arith", "read_after_write", "mul_add", "store_reorder"];
const ORDER_B: &[&str] = &["arith", "mul_add", "store_reorder", "read_after_write"];

fn generated_params() -> Params {
    Params {
        seed: 1,
        steps: 60,
        weights: memory_weights(),
        optional_rules: &[],
        addresses: 8,
    }
}

fn run_rule_order(rulesets: &[&str]) -> (usize, String) {
    let (m, root, _) = generated::build_with_rules(generated_params(), rulesets);
    let algorithm = algorithms::by_name("maxsat.inc_scc").unwrap();
    let extraction = MemIndex::extract(
        &m,
        root,
        (m.init, m.store),
        (
            m.cst, m.var, m.load, m.one, m.inc, m.add, m.sub, m.neg, m.mul, m.square,
        ),
        &algorithm.options(),
        None,
    )
    .unwrap();
    (extraction.cost, linearize_mem(&m, &extraction.index, root))
}

/// Different saturation orders over the same program must reach the same
/// optimum, but may select different linearizations. Each order stays stable
/// when repeated.
#[test]
fn rule_order_can_change_extraction() {
    let first_a = run_rule_order(ORDER_A);
    let first_b = run_rule_order(ORDER_B);

    assert_eq!(
        first_a.0, 51,
        "fixture cost moved; update the expected value"
    );
    assert_eq!(first_b.0, 51, "rule order changed the optimum");
    assert_ne!(
        first_a.1, first_b.1,
        "rule order stopped changing the linearization; the fixture no longer discriminates"
    );

    for _ in 0..3 {
        assert_eq!(run_rule_order(ORDER_A), first_a);
        assert_eq!(run_rule_order(ORDER_B), first_b);
    }
}
