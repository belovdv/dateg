//! End-to-end smoke test for the imprecise SAT + cost-bound baseline.
//!
//! The solver is bench-only in the matrix, so this guards the index/cost
//! contract and the optimality stats on a small solvable generated program.

use std::time::Duration;

use dateg_extractors::dag::{ExtractStats, StatValue};
use dateg_extractors_testkit::algorithms;
use dateg_extractors_testkit::indexes::mem::extract_mem_with_stats;
use dateg_extractors_testkit::sources::generated::{self, Params, memory_weights, rules_by_name};

#[test]
fn sat_baseline_improves_and_reports_optimality() {
    let (green, root, _) = generated::build(Params {
        seed: 1,
        steps: 20,
        weights: memory_weights(),
        optional_rules: rules_by_name("reorder"),
        addresses: generated::DEFAULT_ADDRESSES,
    });

    let mut algorithm = algorithms::by_name("sat.descending").unwrap().clone();
    algorithm.config.timeout = Some(Duration::from_secs(10));

    let mut stats = ExtractStats::default();
    let cost = extract_mem_with_stats(&green, root, &algorithm, Some(&mut stats)).unwrap();
    assert_eq!(cost, 25);

    let count = |key: &str| match &stats.custom[key] {
        StatValue::Count(value) => *value,
        other => panic!("{key}: expected count, got {other:?}"),
    };
    let text = |key: &str| match &stats.custom[key] {
        StatValue::Text(value) => value.as_str(),
        other => panic!("{key}: expected text, got {other:?}"),
    };
    let stats_key = |suffix: &str| format!("sat.descending.{suffix}");
    assert_eq!(text("solver.name"), "sat.descending");
    assert_eq!(text(&stats_key("cycle_blocking")), "shortest");
    assert_eq!(text(&stats_key("descent_fraction")), "0.015625");
    let initial = count(&stats_key("initial_cost"));
    assert!(
        initial > cost as u64,
        "initial {initial} did not improve on final {cost}"
    );
    assert_eq!(count(&stats_key("final_cost")), cost as u64);
    assert_eq!(count(&stats_key("lower_bound")), cost as u64);
    assert_eq!(count(&stats_key("optimal")), 1);
    assert_eq!(count(&stats_key("timed_out")), 0);
    assert!(count(&stats_key("probes")) >= 1);
}
