use std::collections::{BTreeSet, HashSet};

use dateg::{TokenOpaque, execute};

use crate::case::{Case, Complexity, Expected, Purpose, SourceKind, Tier};
use crate::indexes::expr::{extract_expr, extract_expr_eval};
use crate::memory::{Int, Memory, inc_chain, saturate};

/// Builds a case from a fixture generator.
///
/// The fixture is evaluated once at construction to obtain the expected cost,
/// then re-evaluated inside each algorithm run. `Memory` is not captured so the
/// closure stays `Send + Sync` and each algorithm gets a fresh egraph.
macro_rules! case {
    ($name:literal, $tier:ident, $fixture:expr) => {{
        let (_, _, expected, size) = $fixture;
        Case::new(
            $name,
            Purpose::Verification,
            SourceKind::Special,
            Complexity::new(Tier::$tier, size),
            Expected::Exact(expected),
            move |algorithm| {
                let (memory, root, _, _) = $fixture;
                extract_expr(&memory, root, algorithm)
            },
        )
    }};
}

/// A shared `inc` chain used twice by one `mul`: the second use is free, so the
/// cost is `len` for the chain plus one for the mul.
fn diamond(len: usize) -> (Memory, TokenOpaque<Int>, usize, usize) {
    let (mut m, shared) = inc_chain(len);
    let root = m.add_mul(shared, shared).canon(&m.eg);
    let size = m.term_count();
    (m, root, len + 1, size)
}

/// Builds `depth` layers of `width` nodes from the previous layer, alternating
/// `inc` (unary) and `mul` (binary, reusing a sibling). The last layer is folded
/// with `mul`. No rules run, so the exact cost is the number of distinct
/// constructed values reachable from the root.
fn layered(depth: usize, width: usize) -> (Memory, TokenOpaque<Int>, usize, usize) {
    let mut m = Memory::default();
    let mut previous = vec![m.add_one()];
    let mut seen = HashSet::from([previous[0]]);

    for level in 0..depth {
        let mut next = Vec::with_capacity(width);
        for i in 0..width {
            let a = previous[i % previous.len()];
            let value = if (level + i) % 2 == 0 {
                m.add_inc(a)
            } else {
                m.add_mul(a, previous[(i + 1) % previous.len()])
            };
            seen.insert(value);
            next.push(value);
        }
        previous = next;
    }

    let mut root = previous[0];
    for &value in &previous[1..] {
        root = m.add_mul(root, value);
        seen.insert(root);
    }
    let root = root.canon(&m.eg);
    let size = m.term_count();
    (m, root, seen.len(), size)
}

/// Independent DP oracle for the smallest term count of each value using
/// `one`, `inc`, and `mul` (same oracle as the removed dag_correctness test).
fn nat_costs(max_value: usize, max_cost: usize) -> Vec<usize> {
    let mut costs = vec![usize::MAX; max_value + 1];
    costs[1] = 1;
    let mut available: HashSet<BTreeSet<usize>> = [[1].into_iter().collect()].into_iter().collect();

    for len in 2..=max_cost {
        let mut next = HashSet::new();
        for available in available {
            let mut values = HashSet::new();
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
                    costs[value] = costs[value].min(len);
                }
            }
        }
        available = next;
    }
    costs
}

fn nat_case(name: &'static str, tier: Tier, max_value: usize, max_cost: usize) -> Case {
    let costs = nat_costs(max_value, max_cost);
    assert!(costs.iter().skip(1).all(|&cost| cost != usize::MAX));
    Case::new(
        name,
        Purpose::Verification,
        SourceKind::Special,
        Complexity::new(tier, (max_value + 2).max(20)),
        Expected::Exact(costs[max_value]),
        move |algorithm| {
            let (mut m, _) = inc_chain(max_value);
            saturate(&mut m, &["tree"]);
            for (value, expected_cost) in costs.iter().enumerate().take(max_value + 1).skip(1) {
                let root = m.get_val(value).canon(&m.eg);
                let (cost, evaluated) = extract_expr_eval(&m, root, algorithm)?;
                assert_eq!(cost, *expected_cost, "value {value}");
                assert_eq!(evaluated, value, "value {value}");
            }
            Ok(costs[max_value])
        },
    )
}

pub fn cases() -> Vec<Case> {
    vec![
        case!("special/chain_6", Fast, {
            let (m, root) = inc_chain(6);
            let size = m.term_count();
            (m, root, 6, size)
        }),
        case!("special/chain_9", Fast, {
            let (m, root) = inc_chain(9);
            let size = m.term_count();
            (m, root, 9, size)
        }),
        case!("special/diamond_3", Fast, diamond(3)),
        case!("special/diamond_6", Fast, diamond(6)),
        case!("special/layered_3x2", Fast, layered(3, 2)),
        case!("special/layered_4x2", Fast, layered(4, 2)),
        case!("special/chain_16", Slow, {
            let (m, root) = inc_chain(16);
            let size = m.term_count();
            (m, root, 16, size)
        }),
        case!("special/chain_64", Slow, {
            let (m, root) = inc_chain(64);
            let size = m.term_count();
            (m, root, 64, size)
        }),
        case!("special/chain_256", Slow, {
            let (m, root) = inc_chain(256);
            let size = m.term_count();
            (m, root, 256, size)
        }),
        case!("special/diamond_16", Slow, diamond(16)),
        case!("special/diamond_32", Slow, diamond(32)),
        case!("special/layered_6x3", Slow, layered(6, 3)),
        case!("special/layered_8x4", Slow, layered(8, 4)),
        case!("special/layered_12x6", Slow, layered(12, 6)),
        nat_case("special/nat_range_8", Tier::Slow, 8, 9),
        nat_case("special/nat_range_15", Tier::Slow, 15, 9),
        nat_case("special/nat_range_31", Tier::Slow, 31, 12),
        Case::new(
            "special/many_roots_loops",
            Purpose::Verification,
            SourceKind::Special,
            Complexity::new(Tier::Slow, 20),
            Expected::Exact(16),
            |algorithm| {
                let mut m = Memory::default();
                let one = m.add_one();
                let mut roots = vec![one];
                let mut last = one;
                for _ in 1..16 {
                    last = m.add_inc(last);
                    roots.push(last);
                }
                let loop_root = m.add_inc(last);
                let e2 = m.add_inc(one);
                execute! {m;
                    (rule (uni {loop_root} {e2}))
                }
                while m.run_ruleset("") {}

                for (index, root) in roots.iter().enumerate() {
                    let root = root.canon(&m.eg);
                    let cost = extract_expr(&m, root, algorithm)?;
                    assert_eq!(cost, index + 1, "root value {}", index + 1);
                }
                let loop_root = loop_root.canon(&m.eg);
                assert_eq!(extract_expr(&m, loop_root, algorithm)?, 2);
                Ok(16)
            },
        ),
    ]
}
