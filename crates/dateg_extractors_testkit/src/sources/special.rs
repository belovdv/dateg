use std::collections::HashSet;

use dateg::TokenOpaque;

use crate::case::{Case, Complexity, Expected, Purpose, SourceKind, Tier};
use crate::indexes::expr::extract_expr;
use crate::memory::{Int, Memory, inc_chain};

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
    ]
}
