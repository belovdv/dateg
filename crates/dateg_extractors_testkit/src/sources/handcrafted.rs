use std::time::Duration;

use dateg::{TokenOpaque, execute};
use dateg_extractors::dag::ExtractStats;

use crate::case::{Case, Complexity, Expected, Purpose, SourceKind, Tier};
use crate::indexes::container::{extract_container, extract_filtered_container};
use crate::indexes::expr::{
    extract_expr, extract_expr_string, extract_expr_with, extract_full_expr, extract_loop,
    extract_zero_expr,
};
use crate::memory::{Int, Memory, inc_chain, saturate};

macro_rules! case {
    ($name:literal, $tier:ident, $expected:expr, $run:expr) => {{
        let expected = $expected;
        Case::new(
            $name,
            Purpose::Verification,
            SourceKind::Handcrafted,
            expected.complexity_hint(Tier::$tier),
            expected,
            $run,
        )
    }};
}

fn neg_loop(len: usize) -> (Memory, TokenOpaque<Int>, usize) {
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
    let size = m.term_count();
    (m, root, size)
}

pub fn cases() -> Vec<Case> {
    let (_, _, loop_size) = neg_loop(16);
    let cases = vec![
        case!(
            "hand/basic_inc_chain",
            Fast,
            Expected::Exact(4),
            |algorithm| {
                let (m, root) = inc_chain(4);
                extract_expr(&m, root, algorithm)
            }
        ),
        case!("hand/full_square", Fast, Expected::Exact(3), |algorithm| {
            let mut m = Memory::default();
            let one = m.add_one();
            let two = m.add_inc(one);
            let root = m.add_square(two).canon(&m.eg);
            extract_full_expr(&m, root, algorithm)
        }),
        case!(
            "hand/zero_cost_tie",
            Fast,
            Expected::Exact(0),
            |algorithm| {
                let (m, root) = inc_chain(4);
                extract_zero_expr(&m, root, algorithm)
            }
        ),
        case!(
            "hand/zero_cost_exclusive",
            Fast,
            Expected::Exact(0),
            |algorithm| {
                let mut m = Memory::default();
                let one = m.add_one();
                let two = m.add_inc(one);
                let square = m.add_square(two);
                let mul = m.add_mul(two, two);
                saturate(&mut m, &["tree"]);
                let root = square.canon(&m.eg);
                assert_eq!(root, mul.canon(&m.eg));
                // Exclusivity across constructor tables is a graph-level
                // invariant, but selection is nondeterministic; repeat so a
                // missing conflict group is caught reliably.
                let mut cost = 0;
                for _ in 0..64 {
                    cost = extract_zero_expr(&m, root, algorithm)?;
                }
                Ok(cost)
            }
        ),
        case!(
            "hand/stats_populated",
            Fast,
            Expected::Exact(3),
            |algorithm| {
                let (m, root) = inc_chain(3);
                let mut stats = ExtractStats::default();
                let cost = extract_expr_with(&m, root, algorithm, Some(&mut stats))?;
                assert!(stats.solver.is_some());
                assert!(stats.verify.is_some());
                assert!(stats.total > Duration::ZERO);
                Ok(cost)
            }
        ),
        case!("hand/union_loop", Fast, Expected::Exact(2), |algorithm| {
            let mut m = Memory::default();
            let one = m.add_one();
            let e2 = m.add_inc(one);
            let e3 = m.add_inc(e2);
            let x = m.add_inc(e3);
            execute! {m;
                (rule (uni {x} {e2}))
            }
            while m.run_ruleset("") {}
            let root = x.canon(&m.eg);
            extract_expr(&m, root, algorithm)
        }),
        Case::new(
            "hand/neg_loop_long",
            Purpose::Verification,
            SourceKind::Handcrafted,
            Complexity::new(Tier::Slow, loop_size),
            Expected::Exact(17),
            |algorithm| {
                let (m, root, _) = neg_loop(16);
                extract_loop(&m, root, algorithm)
            },
        ),
        case!("hand/neg_loop", Fast, Expected::Exact(2), |algorithm| {
            let mut m = Memory::default();
            let one = m.add_one();
            let a = m.add_neg(one);
            let b = m.add_neg(a);
            execute! {m;
                (rule (uni {a} {b}))
            }
            while m.run_ruleset("") {}
            let root = a.canon(&m.eg);
            extract_loop(&m, root, algorithm)
        }),
        case!("hand/sub_loop", Fast, Expected::Exact(3), |algorithm| {
            let mut m = Memory::default();
            let one = m.add_one();
            let e2 = m.add_inc(one);
            let e3 = m.add_inc(e2);
            let a = m.add_sub(e3, one);
            let b = m.add_sub(a, one);
            execute! {m;
                (rule (uni {a} {b}))
                (rule (uni {a} {e3}))
            }
            while m.run_ruleset("") {}
            let root = a.canon(&m.eg);
            extract_loop(&m, root, algorithm)
        }),
        case!("hand/both_loops", Fast, Expected::Exact(4), |algorithm| {
            let mut m = Memory::default();
            let one = m.add_one();
            let e2 = m.add_inc(one);
            let e3 = m.add_inc(e2);
            let a = m.add_sub(e3, one);
            let n = m.add_neg(a);
            execute! {m;
                (rule (uni {a} {n}))
            }
            while m.run_ruleset("") {}
            let root = a.canon(&m.eg);
            extract_loop(&m, root, algorithm)
        }),
        case!("hand/container", Fast, Expected::Exact(1), |algorithm| {
            let mut m = Memory::default();
            let container = m.add_container_value(dateg::ContainerVec(vec![]));
            let root = m.add_expr_v(m.s_one, container).canon(&m.eg);
            extract_container(&m, root, algorithm)
        }),
        case!(
            "hand/container_cost_filter",
            Fast,
            Expected::Exact(1),
            |algorithm| {
                let mut m = Memory::default();
                let container = m.add_container_value(dateg::ContainerVec(vec![]));
                let root = m.add_expr_v(m.s_one, container).canon(&m.eg);
                extract_filtered_container(&m, root, algorithm)
            }
        ),
        case!(
            "hand/container_cost_filtered_out",
            Fast,
            Expected::Failure("solver proved no solution exists"),
            |algorithm| {
                let mut m = Memory::default();
                let container = m.add_container_value(dateg::ContainerVec(vec![]));
                let root = m.add_expr_v(m.s_add, container).canon(&m.eg);
                crate::indexes::container::FilteredContainerIndex::extract(
                    &m,
                    root,
                    m.expr_v,
                    &algorithm.options(),
                    None,
                )
                .map(|extraction| extraction.cost)
            }
        ),
        Case::new(
            "hand/unknown_reference",
            Purpose::Verification,
            SourceKind::Handcrafted,
            crate::case::Complexity::new(Tier::Fast, inc_chain(3).0.term_count()),
            Expected::Unknown,
            |algorithm| {
                let (m, root) = inc_chain(3);
                extract_expr(&m, root, algorithm)
            },
        ),
        Case::new(
            "hand/known_optima_structures",
            Purpose::Verification,
            SourceKind::Handcrafted,
            Complexity::new(Tier::Fast, 18),
            Expected::Exact(6),
            |algorithm| {
                let (mut m, _) = inc_chain(16);
                saturate(&mut m, &["tree"]);
                let cases: &[(usize, usize, &[&str])] = &[
                    (4, 3, &["(mul (inc one) (inc one))"]),
                    (
                        13,
                        6,
                        &[
                            "(inc (mul (inc (inc (inc one))) (inc (inc one))))",
                            "(inc (mul (inc one) (mul (inc (inc one)) (inc one))))",
                            "(inc (mul (inc (inc one)) (mul (inc one) (inc one))))",
                        ],
                    ),
                    (
                        16,
                        4,
                        &["(mul (mul (inc one) (inc one)) (mul (inc one) (inc one)))"],
                    ),
                ];
                for &(value, expected_cost, allowed) in cases {
                    let root = m.get_val(value).canon(&m.eg);
                    let (cost, rendered) = extract_expr_string(&m, root, algorithm)?;
                    assert_eq!(cost, expected_cost, "value {value}");
                    assert!(
                        allowed.contains(&rendered.as_str()),
                        "value {value}: {rendered} not in {allowed:?}"
                    );
                }
                Ok(6)
            },
        ),
        case!(
            "hand/unreachable_pruned",
            Fast,
            Expected::Exact(3),
            |algorithm| {
                let mut m = Memory::default();
                let one = m.add_one();
                let two = m.add_inc(one);
                let root = m.add_inc(two).canon(&m.eg);
                let unreachable = m.add_mul(root, root);
                m.add_square(unreachable);
                extract_expr(&m, root, algorithm)
            }
        ),
        case!(
            "hand/index_square_vs_mul",
            Fast,
            Expected::Exact(3),
            |algorithm| {
                let mut m = Memory::default();
                let one = m.add_one();
                let two = m.add_inc(one);
                let square = m.add_square(two);
                let mul = m.add_mul(two, two);
                saturate(&mut m, &["tree"]);
                let root = square.canon(&m.eg);
                assert_eq!(root, mul.canon(&m.eg));
                extract_expr(&m, root, algorithm)
            }
        ),
    ];
    cases
}
