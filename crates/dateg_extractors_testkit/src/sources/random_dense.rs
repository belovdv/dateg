use dateg::TokenOpaque;
use rand::{Rng, SeedableRng, rngs::StdRng};

use crate::case::{Case, Complexity, Expected, Purpose, SourceKind, Tier};
use crate::indexes::mem::extract_int;
use crate::memory::{Int, Memory, saturate_with_assoc};

/// Builds a parameterized DAG from a value pool: `inc_count` unary nodes, then
/// `mul_count` binary nodes whose operands are drawn from the growing pool, so
/// subexpressions are reused. `tree`/`ac` saturation then adds alternative
/// representations, so the optimal cost is not known in advance. The case is a
/// benchmark; an oracle is used only when its capacity covers
/// the resulting graph.
fn fixture(seed: u64, inc_count: usize, mul_count: usize) -> (Memory, TokenOpaque<Int>, usize) {
    let mut m = Memory::default();
    let mut rng = StdRng::seed_from_u64(seed);
    let one = m.add_one();
    let mut pool = vec![one];

    for _ in 0..inc_count {
        let a = pool[rng.random_range(0..pool.len())];
        let value = m.add_inc(a);
        pool.push(value);
    }
    for _ in 0..mul_count {
        let a = pool[rng.random_range(0..pool.len())];
        let b = pool[rng.random_range(0..pool.len())];
        let value = m.add_mul(a, b);
        pool.push(value);
    }

    let root = *pool.last().unwrap();
    saturate_with_assoc(&mut m, &["tree", "comm"]);
    let root = root.canon(&m.eg);
    let size = m.term_count();
    (m, root, size)
}

fn case(name: &'static str, tier: Tier, seed: u64, inc_count: usize, mul_count: usize) -> Case {
    // Avoid building/saturating slow fixtures just to estimate complexity; they
    // are unambiguously beyond the naive oracle's capacity.
    let size = match tier {
        Tier::Fast => fixture(seed, inc_count, mul_count).2,
        Tier::Slow | Tier::Soak => 1000,
    };
    Case::new(
        name,
        Purpose::Verification,
        SourceKind::RandomDense,
        Complexity::new(tier, size),
        Expected::Unknown,
        move |algorithm| {
            let (m, root, _) = fixture(seed, inc_count, mul_count);
            extract_int(&m, root, algorithm)
        },
    )
}

pub fn cases() -> Vec<Case> {
    vec![
        case("random_dense/ac_inc1_mul1_seed1", Tier::Fast, 1, 1, 1),
        case("random_dense/ac_inc1_mul2_seed1", Tier::Fast, 1, 1, 2),
        case("random_dense/ac_inc2_mul3_seed1", Tier::Slow, 1, 2, 3),
        case("random_dense/ac_inc3_mul4_seed1", Tier::Slow, 1, 3, 4),
        case("random_dense/ac_inc4_mul6_seed1", Tier::Slow, 1, 4, 6),
    ]
}
