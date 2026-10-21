use dateg::TokenOpaque;
use rand::{Rng, SeedableRng, rngs::StdRng};

use crate::case::{Case, Complexity, Expected, Purpose, SourceKind, Tier};
use crate::indexes::expr::extract_expr;
use crate::memory::{Int, Memory};

/// Recursively builds a term tree of the given depth: leaves are `one`; one
/// quarter of internal nodes are `inc`, the rest `mul` (so it is not too sparse).
/// No rules run, but the cost
/// is not precomputed: the oracle derives it when feasible.
fn generate(
    m: &mut Memory,
    rng: &mut StdRng,
    depth: usize,
    one: TokenOpaque<Int>,
) -> TokenOpaque<Int> {
    if depth == 0 {
        return one;
    }
    match rng.random_range(0..4) {
        0 => {
            let a = generate(m, rng, depth - 1, one);
            m.add_inc(a)
        }
        _ => {
            let a = generate(m, rng, depth - 1, one);
            let b = generate(m, rng, depth - 1, one);
            m.add_mul(a, b)
        }
    }
}

/// Builds a deterministic sparse fixture for `seed`.
fn fixture(seed: u64, depth: usize) -> (Memory, TokenOpaque<Int>, usize) {
    let mut m = Memory::default();
    let one = m.add_one();
    let mut rng = StdRng::seed_from_u64(seed);
    let root = generate(&mut m, &mut rng, depth, one);
    let root = root.canon(&m.eg);
    let size = m.term_count();
    (m, root, size)
}

fn case(name: &'static str, tier: Tier, seed: u64, depth: usize) -> Case {
    // Avoid building large slow fixtures just to estimate complexity.
    let size = match tier {
        Tier::Fast => fixture(seed, depth).2,
        Tier::Slow | Tier::Soak => 1000,
    };
    Case::new(
        name,
        Purpose::Verification,
        SourceKind::RandomSparse,
        Complexity::new(tier, size),
        Expected::Unknown,
        move |algorithm| {
            let (m, root, _) = fixture(seed, depth);
            extract_expr(&m, root, algorithm)
        },
    )
}

pub fn cases() -> Vec<Case> {
    vec![
        case("random_sparse/depth4_seed1", Tier::Fast, 1, 4),
        case("random_sparse/depth4_seed2", Tier::Fast, 2, 4),
        case("random_sparse/depth8_seed1", Tier::Slow, 1, 8),
        case("random_sparse/depth8_seed2", Tier::Slow, 2, 8),
        case("random_sparse/depth12_seed1", Tier::Slow, 1, 12),
        case("random_sparse/depth16_seed1", Tier::Slow, 1, 16),
    ]
}
