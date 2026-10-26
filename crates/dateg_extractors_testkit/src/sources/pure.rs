//! Pure programs with free inputs.
//!
//! The generator builds `vars` variable leaves, a body of pure integer
//! operations, and exactly one store at the end. There is no initial store or
//! load, so all of the extraction difficulty is in the pure cone. Variables are
//! leaves that can be selected for free; the remaining cost is the initial
//! memory node, the final store, and the arithmetic operations needed to reach
//! its value.

use dateg::TokenOpaque;
use rand::{SeedableRng, rngs::StdRng};

use crate::memory::{Int, Mem, Memory, saturate, saturate_with_assoc};

use super::generated::{BASE_RULES, Op, Weights, pick_int, pick_op};

#[derive(Clone, Copy)]
pub struct Params {
    pub seed: u64,
    /// Number of pure operations. The graph is saturated with the ordinary
    /// convergence guard; with `assoc`, some seeds do not terminate, so bench
    /// params are chosen experimentally rather than by a row budget.
    pub steps: usize,
    /// Number of variable leaves. The first variable is used as the store
    /// address, so it must be positive.
    pub vars: usize,
    pub weights: Weights,
    pub optional_rules: &'static [&'static str],
}

pub fn build(params: Params) -> (Memory, TokenOpaque<Mem>, usize) {
    assert!(
        params.vars > 0,
        "a pure program needs at least one variable"
    );
    assert_eq!(
        params.weights.store, 0,
        "pure programs cannot generate stores"
    );
    assert_eq!(
        params.weights.load, 0,
        "pure programs cannot generate loads"
    );

    let mut m = Memory::default();
    let init = m.add_init();
    let mut rng = StdRng::seed_from_u64(params.seed);
    let mut pool: Vec<TokenOpaque<Int>> = Vec::with_capacity(params.vars + params.steps);
    for i in 0..params.vars {
        let table = m.var;
        let name = m.add_primitive_value(format!("x{i}"));
        pool.push(m.row_add(table, (name,)));
    }

    for _ in 0..params.steps {
        let op = pick_op(&mut rng, params.weights);
        let a = pick_int(&mut rng, params.weights, &pool);
        let value = match op {
            Op::Add => {
                let b = pick_int(&mut rng, params.weights, &pool);
                m.add_add(a, b)
            }
            Op::Sub => {
                let b = pick_int(&mut rng, params.weights, &pool);
                m.add_sub(a, b)
            }
            Op::Mul => {
                let b = pick_int(&mut rng, params.weights, &pool);
                m.add_mul(a, b)
            }
            Op::Neg => m.add_neg(a),
            Op::Inc => m.add_inc(a),
            Op::Square => m.add_square(a),
            Op::Store | Op::Load => unreachable!("pure weights exclude memory operations"),
        };
        pool.push(value);
    }

    let assoc = params.optional_rules.contains(&"assoc");
    let mut rules = BASE_RULES.to_vec();
    rules.extend(
        params
            .optional_rules
            .iter()
            .copied()
            .filter(|rule| *rule != "assoc"),
    );
    if assoc {
        saturate_with_assoc(&mut m, &rules);
    } else {
        saturate(&mut m, &rules);
    }

    let addr = pool[0].canon(&m.eg);
    let value = pool.last().unwrap().canon(&m.eg);
    let root = m.add_store(init, addr, value).canon(&m.eg);
    let size = m.term_count();
    (m, root, size)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::generated::{pure_weights, rules_by_name};

    fn params(seed: u64, steps: usize, vars: usize) -> Params {
        Params {
            seed,
            steps,
            vars,
            weights: pure_weights(),
            optional_rules: rules_by_name("none"),
        }
    }

    #[test]
    fn one_store_and_no_loads() {
        let mut checked = 0;
        for optional_rules in [rules_by_name("none"), rules_by_name("assoc")] {
            let params = Params {
                optional_rules,
                ..params(1, 20, 4)
            };
            let (m, root, _) = build(params);
            let mut stores = 0;
            m.eg.for_each_row(m.store, |_, _| stores += 1);
            let mut loads = 0;
            m.eg.for_each_row(m.load, |_, _| loads += 1);
            assert_eq!(stores, 1, "{optional_rules:?}");
            assert_eq!(loads, 0, "{optional_rules:?}");

            let algorithm = crate::algorithms::by_name("maxsat.inc_scc")
                .expect("maxsat")
                .clone();
            assert!(
                crate::indexes::mem::extract_mem(&m, root, &algorithm).is_ok(),
                "{optional_rules:?}"
            );
            checked += 1;
        }
        assert_eq!(checked, 2);
    }

    #[test]
    fn dimensions_are_deterministic() {
        let params = Params {
            seed: 1,
            steps: 20,
            vars: 4,
            weights: pure_weights(),
            optional_rules: rules_by_name("assoc"),
        };
        let build_once = || {
            let (m, _, size) = build(params);
            (size, m.term_count())
        };
        assert_eq!(build_once(), build_once());
    }
}
