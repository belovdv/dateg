use dateg::{TokenOpaque, execute};
use dateg_extractors_testkit::algorithms::{self, Algorithm};
use dateg_extractors_testkit::indexes::expr::{extract_expr_string, extract_loop_string};
use dateg_extractors_testkit::memory::{Int, Memory, inc_chain};
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
