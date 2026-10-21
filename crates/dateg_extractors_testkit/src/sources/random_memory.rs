use dateg::TokenOpaque;
use rand::{Rng, SeedableRng, rngs::StdRng};

use crate::case::{Case, Complexity, Expected, Purpose, SourceKind, Tier};
use crate::indexes::mem::{extract_int, extract_mem};
use crate::memory::{Int, Mem, Memory, saturate};

const ADDRS: usize = 6;
const VALUES: usize = 10;

fn stores(seed: u64, count: usize) -> (Memory, TokenOpaque<Mem>, usize) {
    let mut m = Memory::default();
    let mut rng = StdRng::seed_from_u64(seed);
    let mut mem = m.add_init();
    for _ in 0..count {
        let addr = m.add_cst(rng.random_range(0..ADDRS));
        let value = m.add_cst(rng.random_range(0..VALUES));
        mem = m.add_store(mem, addr, value);
    }
    saturate(&mut m, &["alias"]);
    let mem = mem.canon(&m.eg);
    let size = m.term_count();
    (m, mem, size)
}

fn read_write(
    seed: u64,
    store_count: usize,
    load_count: usize,
) -> (Memory, TokenOpaque<Int>, usize) {
    let mut m = Memory::default();
    let mut rng = StdRng::seed_from_u64(seed);
    let mut mem = m.add_init();
    for _ in 0..store_count {
        let addr = m.add_cst(rng.random_range(0..ADDRS));
        let value = m.add_cst(rng.random_range(0..VALUES));
        mem = m.add_store(mem, addr, value);
    }
    let addr = m.add_cst(rng.random_range(0..ADDRS));
    let mut result = m.add_load(mem, addr);
    for _ in 1..load_count {
        let addr = m.add_cst(rng.random_range(0..ADDRS));
        let load = m.add_load(mem, addr);
        result = m.add_add(result, load);
    }
    saturate(&mut m, &["alias"]);
    let result = result.canon(&m.eg);
    let size = m.term_count();
    (m, result, size)
}

fn write_case(name: &'static str, tier: Tier, seed: u64, count: usize) -> Case {
    // Slow fixtures can be expensive to saturate; avoid building them at case
    // construction and mark them conservatively above the naive threshold.
    let size = match tier {
        Tier::Fast => stores(seed, count).2,
        Tier::Slow | Tier::Soak => 1000,
    };
    Case::new(
        name,
        Purpose::Verification,
        SourceKind::RandomMemory,
        Complexity::new(tier, size),
        Expected::Unknown,
        move |algorithm| {
            let (m, root, _) = stores(seed, count);
            extract_mem(&m, root, algorithm)
        },
    )
}

fn read_write_case(
    name: &'static str,
    tier: Tier,
    seed: u64,
    store_count: usize,
    load_count: usize,
) -> Case {
    let size = match tier {
        Tier::Fast => read_write(seed, store_count, load_count).2,
        Tier::Slow | Tier::Soak => 1000,
    };
    Case::new(
        name,
        Purpose::Verification,
        SourceKind::RandomMemory,
        Complexity::new(tier, size),
        Expected::Unknown,
        move |algorithm| {
            let (m, root, _) = read_write(seed, store_count, load_count);
            extract_int(&m, root, algorithm)
        },
    )
}

pub fn cases() -> Vec<Case> {
    vec![
        write_case("random_memory/write_only_small", Tier::Fast, 1, 2),
        read_write_case("random_memory/read_write_small", Tier::Fast, 1, 2, 1),
        write_case("random_memory/write_only_seed1", Tier::Fast, 1, 4),
        write_case("random_memory/write_only_seed2", Tier::Fast, 2, 4),
        read_write_case("random_memory/read_write_seed1", Tier::Fast, 1, 4, 3),
        read_write_case("random_memory/read_write_seed2", Tier::Fast, 2, 4, 3),
        write_case("random_memory/write_only_8", Tier::Slow, 1, 8),
        read_write_case("random_memory/read_write_12x6", Tier::Slow, 1, 12, 6),
    ]
}
