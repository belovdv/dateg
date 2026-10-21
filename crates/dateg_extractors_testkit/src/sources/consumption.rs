use dateg::TokenOpaque;

use crate::case::{Case, Complexity, Expected, Purpose, SourceKind, Tier};
use crate::indexes::mem::{extract_int, extract_mem};
use crate::memory::{Int, Mem, Memory, saturate};

macro_rules! case {
    ($name:literal, $cost:expr, $run:expr) => {{
        let expected = Expected::Exact($cost);
        Case::new(
            $name,
            Purpose::Verification,
            SourceKind::Consumption,
            expected.complexity_hint(Tier::Fast),
            expected,
            $run,
        )
    }};
}

fn load_only() -> (Memory, TokenOpaque<Int>, usize) {
    let mut m = Memory::default();
    let init = m.add_init();
    let addr = m.add_cst(1usize);
    let root = m.add_load(init, addr);
    saturate(&mut m, &["alias"]);
    let root = root.canon(&m.eg);
    let size = m.term_count();
    (m, root, size)
}

fn store_loaded_value() -> (Memory, TokenOpaque<Mem>, usize) {
    let mut m = Memory::default();
    let init = m.add_init();
    let addr = m.add_cst(1usize);
    let value = m.add_cst(5usize);
    let stored = m.add_store(init, addr, value);
    let loaded = m.add_load(stored, addr);
    let root = m.add_store(stored, addr, loaded);
    saturate(&mut m, &["alias"]);
    let root = root.canon(&m.eg);
    let size = m.term_count();
    (m, root, size)
}

fn forwarding_expressions() -> (Memory, TokenOpaque<Mem>, usize) {
    let mut m = Memory::default();
    let init = m.add_init();
    let p = m.add_cst(7usize);
    let c1 = m.add_cst(1usize);
    let c5 = m.add_cst(5usize);
    let c6 = m.add_cst(6usize);
    let p_a1 = m.add_add(p, c1);
    let s1 = m.add_store(init, p_a1, c1);
    let p_s1 = m.add_sub(p, c1);
    let s2 = m.add_store(s1, p_s1, c5);
    let x = m.add_load(s2, p_a1);
    let p_ax = m.add_add(p, x);
    let root = m.add_store(s2, p_ax, c6);
    saturate(&mut m, &["alias"]);
    let root = root.canon(&m.eg);
    let size = m.term_count();
    (m, root, size)
}

pub fn cases() -> Vec<Case> {
    let (_, _, load_size) = load_only();
    let (_, _, store_loaded_size) = store_loaded_value();
    let (_, _, forwarding_size) = forwarding_expressions();
    let forwarding_size = forwarding_size.max(15);
    vec![
        Case::new(
            "consumption/load_only",
            Purpose::Verification,
            SourceKind::Consumption,
            Complexity::new(Tier::Fast, load_size),
            Expected::Unknown,
            |algorithm| {
                let (m, root, _) = load_only();
                extract_int(&m, root, algorithm)
            },
        ),
        Case::new(
            "consumption/store_loaded_value_noop",
            Purpose::Verification,
            SourceKind::Consumption,
            Complexity::new(Tier::Fast, 15),
            Expected::Unknown,
            |algorithm| {
                let mut base = Memory::default();
                let init = base.add_init();
                let addr = base.add_cst(1usize);
                let value = base.add_cst(5usize);
                let stored = base.add_store(init, addr, value);
                saturate(&mut base, &["alias"]);
                let stored = stored.canon(&base.eg);
                let baseline = extract_mem(&base, stored, algorithm)?;

                let (m, root, _) = store_loaded_value();
                let noop = extract_mem(&m, root, algorithm)?;
                assert_eq!(noop, baseline, "store of loaded value must not add cost");
                Ok(noop)
            },
        ),
        Case::new(
            "consumption/forwarding_expressions",
            Purpose::Verification,
            SourceKind::Consumption,
            Complexity::new(Tier::Fast, forwarding_size),
            Expected::Unknown,
            |algorithm| {
                let (m, root, _) = forwarding_expressions();
                extract_mem(&m, root, algorithm)
            },
        ),
        Case::new(
            "consumption/store_loaded_value",
            Purpose::Verification,
            SourceKind::Consumption,
            Complexity::new(Tier::Fast, store_loaded_size),
            Expected::Unknown,
            |algorithm| {
                let (m, root, _) = store_loaded_value();
                extract_mem(&m, root, algorithm)
            },
        ),
        case!("consumption/read_after_write", 1, |algorithm| {
            let mut m = Memory::default();
            let init = m.add_init();
            let addr = m.add_cst(1usize);
            let val = m.add_cst(5usize);
            let stored = m.add_store(init, addr, val);
            let root = m.add_load(stored, addr);
            saturate(&mut m, &["alias"]);
            let root = root.canon(&m.eg);
            extract_int(&m, root, algorithm)
        }),
        case!("consumption/unrelated_store", 3, |algorithm| {
            let mut m = Memory::default();
            let init = m.add_init();
            let addr1 = m.add_cst(1usize);
            let addr2 = m.add_cst(2usize);
            let val = m.add_cst(5usize);
            let stored = m.add_store(init, addr1, val);
            let root = m.add_load(stored, addr2);
            saturate(&mut m, &["alias"]);
            let root = root.canon(&m.eg);
            extract_int(&m, root, algorithm)
        }),
        case!("consumption/store_merge", 4, |algorithm| {
            let mut m = Memory::default();
            let init = m.add_init();
            let addr = m.add_cst(1usize);
            let val1 = m.add_cst(5usize);
            let val2 = m.add_cst(7usize);
            let stored1 = m.add_store(init, addr, val1);
            let root = m.add_store(stored1, addr, val2);
            saturate(&mut m, &["alias"]);
            let root = root.canon(&m.eg);
            extract_mem(&m, root, algorithm)
        }),
        case!("consumption/store_reorder", 7, |algorithm| {
            let mut m = Memory::default();
            let init = m.add_init();
            let addr1 = m.add_cst(1usize);
            let addr2 = m.add_cst(2usize);
            let val1 = m.add_cst(5usize);
            let val2 = m.add_cst(7usize);
            let stored1 = m.add_store(init, addr1, val1);
            let root = m.add_store(stored1, addr2, val2);
            saturate(&mut m, &["alias"]);
            let root = root.canon(&m.eg);
            extract_mem(&m, root, algorithm)
        }),
    ]
}
