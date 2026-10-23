//! Graded extraction benchmark set.
//!
//! Cases are ordered by measured reference time (single `maxsat.inc_scc` run on
//! the development machine, mostly under 12-way load); the intent is a roughly
//! log-uniform ladder from a few seconds to an hour. `reference_seconds` is
//! descriptive only; the bench binary re-measures everything.
//!
//! These cases are too slow for the test matrix. Use the `bench` binary.

use crate::sources::generated::{DEFAULT_ADDRESSES, Params, memory_weights, rules_by_name};

pub struct BenchCase {
    pub name: &'static str,
    pub params: Params,
    /// `None` for the top rungs: the reference did not finish in budget, so the
    /// optimum is unknown and the bench can only check that a solution is valid.
    pub expected_cost: Option<usize>,
    /// Measured with the reference maxsat implementation on the development
    /// machine, mostly under 12-way load.
    pub reference_seconds: f64,
    /// False when the reference was stopped before finishing, making
    /// `reference_seconds` a lower bound rather than a measurement.
    pub reference_exact: bool,
}

fn case(
    name: &'static str,
    seed: u64,
    steps: usize,
    rules: &'static str,
    addresses: usize,
    expected_cost: usize,
    reference_seconds: f64,
) -> BenchCase {
    BenchCase {
        name,
        params: Params {
            seed,
            steps,
            weights: memory_weights(),
            optional_rules: rules_by_name(rules),
            addresses,
        },
        expected_cost: Some(expected_cost),
        reference_seconds,
        reference_exact: true,
    }
}

/// Unmeasured case used to widen the A/B corpus (AC, reordering, longer
/// programs). No reference cost; a valid solution counts as a pass, as with the
/// top-rung open cases. The reference time is infinite so these sort last.
fn coverage_case(name: &'static str, seed: u64, steps: usize, rules: &'static str) -> BenchCase {
    BenchCase {
        name,
        params: Params {
            seed,
            steps,
            weights: memory_weights(),
            optional_rules: rules_by_name(rules),
            addresses: DEFAULT_ADDRESSES,
        },
        expected_cost: None,
        reference_seconds: f64::INFINITY,
        reference_exact: false,
    }
}

/// A rung the reference could not finish: no known optimum, only a lower bound
/// on its runtime. Pass means "returned a valid solution"; optimality is not
/// checked because there is no reference answer.
fn open_case(
    name: &'static str,
    seed: u64,
    steps: usize,
    rules: &'static str,
    addresses: usize,
    reference_seconds: f64,
) -> BenchCase {
    BenchCase {
        name,
        params: Params {
            seed,
            steps,
            weights: memory_weights(),
            optional_rules: rules_by_name(rules),
            addresses,
        },
        expected_cost: None,
        reference_seconds,
        reference_exact: false,
    }
}

/// Ordered ladder; keep ascending reference time so prefix runs are meaningful.
///
/// Cases are prefixes of the same generated program when they share a seed, so
/// neighbours are correlated. The set deliberately spans several seeds.
pub fn cases() -> Vec<BenchCase> {
    vec![
        case(
            "reorder_s1_132",
            1,
            132,
            "reorder",
            DEFAULT_ADDRESSES,
            100,
            3.01,
        ),
        case(
            "reorder_s17_154",
            17,
            154,
            "reorder",
            DEFAULT_ADDRESSES,
            128,
            8.35,
        ),
        case(
            "reorder_s17_148",
            17,
            148,
            "reorder",
            DEFAULT_ADDRESSES,
            122,
            10.32,
        ),
        case(
            "reorder_s17_133",
            17,
            133,
            "reorder",
            DEFAULT_ADDRESSES,
            110,
            15.08,
        ),
        case(
            "reorder_s5_138",
            5,
            138,
            "reorder",
            DEFAULT_ADDRESSES,
            123,
            19.92,
        ),
        case(
            "reorder_s5_151",
            5,
            151,
            "reorder",
            DEFAULT_ADDRESSES,
            137,
            21.02,
        ),
        case(
            "reorder_s7_148",
            7,
            148,
            "reorder",
            DEFAULT_ADDRESSES,
            126,
            25.66,
        ),
        case(
            "reorder_s15_158",
            15,
            158,
            "reorder",
            DEFAULT_ADDRESSES,
            124,
            30.82,
        ),
        case(
            "reorder_s7_132",
            7,
            132,
            "reorder",
            DEFAULT_ADDRESSES,
            114,
            31.28,
        ),
        case(
            "reorder_s15_145",
            15,
            145,
            "reorder",
            DEFAULT_ADDRESSES,
            114,
            35.03,
        ),
        case(
            "reorder_s15_138",
            15,
            138,
            "reorder",
            DEFAULT_ADDRESSES,
            111,
            38.01,
        ),
        case(
            "reorder_s17_158",
            17,
            158,
            "reorder",
            DEFAULT_ADDRESSES,
            130,
            65.28,
        ),
        case(
            "reorder_s6_136",
            6,
            136,
            "reorder",
            DEFAULT_ADDRESSES,
            111,
            70.73,
        ),
        case(
            "reorder_s6_158",
            6,
            158,
            "reorder",
            DEFAULT_ADDRESSES,
            125,
            76.55,
        ),
        case(
            "reorder_s6_145",
            6,
            145,
            "reorder",
            DEFAULT_ADDRESSES,
            116,
            105.18,
        ),
        case(
            "reorder_s6_132",
            6,
            132,
            "reorder",
            DEFAULT_ADDRESSES,
            104,
            139.77,
        ),
        case(
            "reorder_s6_138",
            6,
            138,
            "reorder",
            DEFAULT_ADDRESSES,
            111,
            160.62,
        ),
        case(
            "reorder_s17_130",
            17,
            130,
            "reorder",
            DEFAULT_ADDRESSES,
            110,
            186.87,
        ),
        case(
            "reorder_s17_132",
            17,
            132,
            "reorder",
            DEFAULT_ADDRESSES,
            110,
            230.57,
        ),
        case(
            "reorder_s6_134",
            6,
            134,
            "reorder",
            DEFAULT_ADDRESSES,
            108,
            342.22,
        ),
        case(
            "reorder_s17_136",
            17,
            136,
            "reorder",
            DEFAULT_ADDRESSES,
            113,
            362.39,
        ),
        case(
            "reorder_s7_170",
            7,
            170,
            "reorder",
            DEFAULT_ADDRESSES,
            144,
            705.02,
        ),
        case(
            "reorder_s6_151",
            6,
            151,
            "reorder",
            DEFAULT_ADDRESSES,
            120,
            846.48,
        ),
        case(
            "reorder_s17_135",
            17,
            135,
            "reorder",
            DEFAULT_ADDRESSES,
            113,
            923.90,
        ),
        case(
            "reorder_s7_154",
            7,
            154,
            "reorder",
            DEFAULT_ADDRESSES,
            130,
            1253.79,
        ),
        // Lower bounds: the reference was stopped after ~23.5 min without
        // finishing, so these mark the top of the scale rather than a measured
        // time. They pass only on a valid (not verified-optimal) solution.
        open_case(
            "reorder_s15_142",
            15,
            142,
            "reorder",
            DEFAULT_ADDRESSES,
            1410.0,
        ),
        open_case(
            "reorder_s19_138",
            19,
            138,
            "reorder",
            DEFAULT_ADDRESSES,
            1410.0,
        ),
        open_case(
            "reorder_s19_158",
            19,
            158,
            "reorder",
            DEFAULT_ADDRESSES,
            1410.0,
        ),
        // Coverage corpus: AC, write reordering, longer programs, and cases
        // where the chain heuristic runs out of rollbacks. No reference cost.
        coverage_case("coverage_ac_s15_60", 15, 60, "assoc_reorder"),
        coverage_case("coverage_ac_s15_100", 15, 100, "assoc_reorder"),
        coverage_case("coverage_ac_s6_100", 6, 100, "assoc_reorder"),
        coverage_case("coverage_ac_s17_100", 17, 100, "assoc_reorder"),
        coverage_case("coverage_ac_s19_100", 19, 100, "assoc_reorder"),
        coverage_case("coverage_ac_s23_100", 23, 100, "assoc_reorder"),
        coverage_case("coverage_assoc_s5_80", 5, 80, "assoc"),
        coverage_case("coverage_assoc_s15_80", 15, 80, "assoc"),
        coverage_case("coverage_reorder_s15_180", 15, 180, "reorder"),
        coverage_case("coverage_reorder_s6_180", 6, 180, "reorder"),
        coverage_case("coverage_reorder_s5_180", 5, 180, "reorder"),
        coverage_case("coverage_reorder_s17_200", 17, 200, "reorder"),
        coverage_case("coverage_reorder_s22_148", 22, 148, "reorder"),
        coverage_case("coverage_reorder_s19_148", 19, 148, "reorder"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ladder_is_sorted_unique_and_flags_are_consistent() {
        let cases = cases();
        assert!(cases.len() >= 20, "ladder shrank to {}", cases.len());

        let mut names = std::collections::BTreeSet::new();
        for case in &cases {
            assert!(names.insert(case.name), "duplicate case name {}", case.name);
        }
        for pair in cases.windows(2) {
            assert!(
                pair[0].reference_seconds <= pair[1].reference_seconds,
                "{} ({}) sorts after {} ({})",
                pair[0].name,
                pair[0].reference_seconds,
                pair[1].name,
                pair[1].reference_seconds
            );
        }
        for case in &cases {
            assert!(
                case.reference_seconds > 0.0,
                "{} has no reference",
                case.name
            );
            assert_eq!(
                case.expected_cost.is_none(),
                !case.reference_exact,
                "{}: open rungs must be the ones without a measured reference",
                case.name
            );
        }
    }
}
