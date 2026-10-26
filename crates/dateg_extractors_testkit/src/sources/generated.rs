use dateg::TokenOpaque;
use rand::{Rng, SeedableRng, rngs::StdRng};

use crate::algorithms;
use crate::case::{Case, Complexity, Expected, Purpose, SourceKind, Tier};
use crate::indexes::mem::extract_mem;
use crate::memory::{Int, Mem, Memory, saturate, saturate_with_assoc};

pub(crate) const BASE_RULES: &[&str] = &["arith", "read_after_write", "mul_add"];

/// Weight order for [`parse_weights`] and the CLI/env spec.
pub const WEIGHT_NAMES: &[&str] = &[
    "add",
    "sub",
    "mul",
    "neg",
    "inc",
    "square",
    "store",
    "load",
    "const_addr",
    "value_addr",
    "chain",
    "random",
];

#[derive(Clone, Copy)]
pub struct Weights {
    pub add: u32,
    pub sub: u32,
    pub mul: u32,
    pub neg: u32,
    pub inc: u32,
    pub square: u32,
    pub store: u32,
    pub load: u32,
    pub const_addr: u32,
    pub value_addr: u32,
    pub chain: u32,
    pub random: u32,
}

pub fn pure_weights() -> Weights {
    Weights {
        add: 4,
        sub: 3,
        mul: 3,
        neg: 1,
        inc: 2,
        square: 1,
        store: 0,
        load: 0,
        const_addr: 1,
        value_addr: 1,
        chain: 1,
        random: 1,
    }
}

pub fn memory_weights() -> Weights {
    Weights {
        add: 1,
        sub: 1,
        mul: 1,
        neg: 0,
        inc: 0,
        square: 0,
        store: 6,
        load: 4,
        const_addr: 7,
        value_addr: 3,
        chain: 1,
        random: 1,
    }
}

impl std::fmt::Display for Weights {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{},{},{},{},{},{},{},{},{},{},{},{}",
            self.add,
            self.sub,
            self.mul,
            self.neg,
            self.inc,
            self.square,
            self.store,
            self.load,
            self.const_addr,
            self.value_addr,
            self.chain,
            self.random
        )
    }
}

pub fn weights_by_name(name: &str) -> Option<Weights> {
    match name {
        "pure" => Some(pure_weights()),
        "memory" => Some(memory_weights()),
        _ => None,
    }
}

/// Parse comma-separated weights in [`WEIGHT_NAMES`] order.
pub fn parse_weights(spec: &str) -> Weights {
    let values: Vec<u32> = spec
        .split(',')
        .map(|value| {
            value
                .trim()
                .parse()
                .unwrap_or_else(|_| panic!("invalid weight {value:?}"))
        })
        .collect();
    assert_eq!(
        values.len(),
        WEIGHT_NAMES.len(),
        "expected {} comma-separated weights in order {:?}",
        WEIGHT_NAMES.len(),
        WEIGHT_NAMES
    );
    Weights {
        add: values[0],
        sub: values[1],
        mul: values[2],
        neg: values[3],
        inc: values[4],
        square: values[5],
        store: values[6],
        load: values[7],
        const_addr: values[8],
        value_addr: values[9],
        chain: values[10],
        random: values[11],
    }
}

/// Rules are static so they can live in [`Params`]. `assoc` is applied through
/// the saturate-with-assoc path; the rest run as ordinary rulesets.
pub const RULE_PROFILES: &[(&str, &[&str])] = &[
    ("none", &[]),
    ("assoc", &["assoc"]),
    ("reorder", &["store_reorder"]),
    ("assoc_reorder", &["assoc", "store_reorder"]),
];

pub fn rules_by_name(name: &str) -> &'static [&'static str] {
    RULE_PROFILES
        .iter()
        .find(|(rule_profile, _)| *rule_profile == name)
        .unwrap_or_else(|| panic!("unknown rules profile {name:?}"))
        .1
}

#[derive(Clone, Copy)]
pub struct Params {
    pub seed: u64,
    pub steps: usize,
    pub weights: Weights,
    pub optional_rules: &'static [&'static str],
    /// Number of constant addresses the generator can store to and load from.
    /// Higher values produce more distinct memory versions after allocation.
    pub addresses: usize,
}

pub const DEFAULT_ADDRESSES: usize = 8;

#[derive(Clone, Copy)]
pub(crate) enum Op {
    Add,
    Sub,
    Mul,
    Neg,
    Inc,
    Square,
    Store,
    Load,
}

pub(crate) fn pick_op(rng: &mut StdRng, w: Weights) -> Op {
    let ops = [
        (Op::Add, w.add),
        (Op::Sub, w.sub),
        (Op::Mul, w.mul),
        (Op::Neg, w.neg),
        (Op::Inc, w.inc),
        (Op::Square, w.square),
        (Op::Store, w.store),
        (Op::Load, w.load),
    ];
    let total: u32 = ops.iter().map(|(_, weight)| *weight).sum();
    assert!(total > 0, "operation weights must not all be zero");
    let mut pick = rng.random_range(0..total);
    for (op, weight) in ops {
        if pick < weight {
            return op;
        }
        pick -= weight;
    }
    unreachable!()
}

pub(crate) fn pick_int(
    rng: &mut StdRng,
    w: Weights,
    ints: &[TokenOpaque<Int>],
) -> TokenOpaque<Int> {
    let total = w.chain + w.random;
    assert!(total > 0, "operand weights must not both be zero");
    if rng.random_range(0..total) < w.chain {
        *ints.last().unwrap()
    } else {
        ints[rng.random_range(0..ints.len())]
    }
}

fn pick_addr(
    rng: &mut StdRng,
    w: Weights,
    ints: &[TokenOpaque<Int>],
    addrs: &[TokenOpaque<Int>],
) -> TokenOpaque<Int> {
    let total = w.const_addr + w.value_addr;
    assert!(total > 0, "address weights must not both be zero");
    if rng.random_range(0..total) < w.const_addr {
        addrs[rng.random_range(0..addrs.len())]
    } else {
        ints[rng.random_range(0..ints.len())]
    }
}

pub fn build(params: Params) -> (Memory, TokenOpaque<Mem>, usize) {
    let assoc = params.optional_rules.contains(&"assoc");
    let mut rules = BASE_RULES.to_vec();
    rules.extend(
        params
            .optional_rules
            .iter()
            .copied()
            .filter(|rule| *rule != "assoc"),
    );
    build_with(params, move |m| {
        if assoc {
            saturate_with_assoc(m, &rules);
        } else {
            saturate(m, &rules);
        }
    })
}

/// Build the same program as [`build`], but saturate with an explicit ruleset
/// order. `assoc` gets no special treatment here; it runs as an ordinary
/// ruleset if included. Determinism tests use this to show that rule order alone
/// can change which extraction is selected.
pub fn build_with_rules(params: Params, rulesets: &[&str]) -> (Memory, TokenOpaque<Mem>, usize) {
    build_with(params, |m| saturate(m, rulesets))
}

fn build_with(
    params: Params,
    run_saturation: impl FnOnce(&mut Memory),
) -> (Memory, TokenOpaque<Mem>, usize) {
    let mut m = Memory::default();
    let mut rng = StdRng::seed_from_u64(params.seed);
    let one = m.add_one();
    let c1 = m.add_cst(1);
    let c2 = m.add_cst(2);
    let addrs: Vec<TokenOpaque<Int>> = (0..params.addresses).map(|i| m.add_cst(i)).collect();
    let mut ints: Vec<TokenOpaque<Int>> = vec![one, c1, c2];
    let mut mem = m.add_init();
    mem = m.add_store(mem, c1, one);
    ints.push(m.add_load(mem, c1));

    for _ in 0..params.steps {
        match pick_op(&mut rng, params.weights) {
            Op::Store => {
                let addr = pick_addr(&mut rng, params.weights, &ints, &addrs);
                let value = pick_int(&mut rng, params.weights, &ints);
                mem = m.add_store(mem, addr, value);
            }
            Op::Load => {
                let addr = pick_addr(&mut rng, params.weights, &ints, &addrs);
                ints.push(m.add_load(mem, addr));
            }
            op => {
                let a = pick_int(&mut rng, params.weights, &ints);
                let value = match op {
                    Op::Add => {
                        let b = pick_int(&mut rng, params.weights, &ints);
                        m.add_add(a, b)
                    }
                    Op::Sub => {
                        let b = pick_int(&mut rng, params.weights, &ints);
                        m.add_sub(a, b)
                    }
                    Op::Mul => {
                        let b = pick_int(&mut rng, params.weights, &ints);
                        m.add_mul(a, b)
                    }
                    Op::Neg => m.add_neg(a),
                    Op::Inc => m.add_inc(a),
                    Op::Square => m.add_square(a),
                    Op::Store | Op::Load => unreachable!(),
                };
                ints.push(value);
            }
        }
    }

    run_saturation(&mut m);

    // Saturation can rewrite the body's last memory and the operand tokens
    // picked from it, so canonicalize them before the final store. Without
    // this the store row has a stale input that is absent from the extractor's
    // value table, and the root ends up with no constructor.
    let mem = mem.canon(&m.eg);
    // Storing back the value just loaded from the same address is a no-op
    // under `read_after_write`; such a program has no root constructor and is
    // unextractable. Fall back to a fresh constant, which cannot be congruent
    // to any loaded value, so every generated program has a real root.
    let addr = pick_addr(&mut rng, params.weights, &ints, &addrs).canon(&m.eg);
    let value = pick_int(&mut rng, params.weights, &ints).canon(&m.eg);
    let mut root_token = m.add_store(mem, addr, value);
    if root_token.canon(&m.eg) == mem {
        let fresh = m.add_cst(1_000_000 + params.seed as usize);
        root_token = m.add_store(mem, addr, fresh);
    }
    let root = root_token.canon(&m.eg);
    let size = m.term_count();
    (m, root, size)
}

fn oracle_cost(name: &str, params: Params) -> (usize, usize) {
    let (m, root, size) = build(params);
    let algorithm = algorithms::by_name("maxsat.inc_scc").expect("maxsat algorithm");
    let cost = extract_mem(&m, root, algorithm).unwrap_or_else(|error| panic!("{name}: {error}"));
    (cost, size)
}

fn case(name: &'static str, params: Params) -> Case {
    let (cost, size) = oracle_cost(name, params);
    Case::new(
        name,
        Purpose::Verification,
        SourceKind::Generated,
        Complexity::new(Tier::Fast, size.max(15)),
        Expected::Exact(cost),
        move |algorithm| {
            let (m, root, _) = build(params);
            extract_mem(&m, root, algorithm)
        },
    )
}

fn pure(seed: u64, steps: usize, optional_rules: &'static [&'static str]) -> Params {
    Params {
        seed,
        steps,
        weights: Weights {
            add: 4,
            sub: 3,
            mul: 3,
            neg: 1,
            inc: 2,
            square: 1,
            store: 0,
            load: 0,
            const_addr: 1,
            value_addr: 1,
            chain: 1,
            random: 1,
        },
        optional_rules,
        addresses: DEFAULT_ADDRESSES,
    }
}

fn memory(seed: u64, steps: usize, optional_rules: &'static [&'static str]) -> Params {
    Params {
        seed,
        steps,
        weights: Weights {
            add: 1,
            sub: 1,
            mul: 1,
            neg: 0,
            inc: 0,
            square: 0,
            store: 6,
            load: 4,
            const_addr: 7,
            value_addr: 3,
            chain: 1,
            random: 1,
        },
        optional_rules,
        addresses: DEFAULT_ADDRESSES,
    }
}

pub fn cases() -> Vec<Case> {
    vec![
        case("generated/base_seed1", pure(1, 3, &[])),
        case("generated/base_memory_seed1", memory(1, 4, &[])),
        case("generated/assoc_seed1", pure(3, 3, &["assoc"])),
        case("generated/assoc_memory_seed1", memory(1, 4, &["assoc"])),
        case("generated/reorder_seed1", memory(1, 4, &["store_reorder"])),
        case(
            "generated/mixed_seed1",
            memory(1, 4, &["assoc", "store_reorder"]),
        ),
    ]
}
