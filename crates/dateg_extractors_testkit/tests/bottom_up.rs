//! End-to-end checks for the bottom-up validity fallback.
//!
//! The fallback is allowed to be suboptimal, but every returned assignment must
//! pass validation. Tiny cases also compare against the exact baseline to make
//! sure the collapse does not pretend to be optimal by accident.

use std::time::Duration;

use dateg_extractors::dag::{ExtractStats, StatValue};
use dateg_extractors_testkit::algorithms;
use dateg_extractors_testkit::indexes::mem::{extract_mem, extract_mem_with_stats};
use dateg_extractors_testkit::sources::generated::{
    self, Params, memory_weights, pure_weights, rules_by_name,
};
use dateg_extractors_testkit::sources::pure;

fn algorithm(name: &str) -> algorithms::Algorithm {
    let mut algorithm = algorithms::by_name(name).unwrap().clone();
    algorithm.config.timeout = Duration::from_secs(10);
    algorithm
}

fn pure_params(seed: u64, steps: usize) -> pure::Params {
    pure::Params {
        seed,
        steps,
        vars: 4,
        weights: pure_weights(),
        optional_rules: rules_by_name("assoc"),
    }
}

#[test]
fn bottom_up_is_valid_on_tiny_pure() {
    for (seed, steps) in [(1, 20), (2, 40), (3, 30)] {
        let (green, root, _) = pure::build(pure_params(seed, steps));
        let optimal = extract_mem(&green, root, &algorithm("maxsat.inc_scc"))
            .unwrap_or_else(|error| panic!("maxsat {seed}/{steps}: {error}"));

        let mut stats = ExtractStats::default();
        let cost = extract_mem_with_stats(
            &green,
            root,
            &algorithm("sat.descending.bottom_up"),
            Some(&mut stats),
        )
        .unwrap_or_else(|error| panic!("bottom-up {seed}/{steps}: {error}"));
        assert!(
            cost >= optimal,
            "bottom-up {cost} is below the optimum {optimal} on {seed}/{steps}"
        );
        assert_eq!(
            stats.custom["filtration.mode"],
            StatValue::Text("component_bottom_up".into())
        );
        assert!(matches!(
            stats.custom["filtration.bottom_up.components"],
            StatValue::Count(count) if count > 0
        ));
    }
}

#[test]
fn bottom_up_is_valid_on_tiny_memory() {
    for (seed, steps, rules) in [(1, 20, "reorder"), (15, 20, "assoc_reorder")] {
        let (green, root, _) = generated::build(Params {
            seed,
            steps,
            weights: memory_weights(),
            optional_rules: rules_by_name(rules),
            addresses: generated::DEFAULT_ADDRESSES,
        });
        let cost = extract_mem(&green, root, &algorithm("sat.descending.bottom_up"))
            .unwrap_or_else(|error| panic!("bottom-up {seed}/{steps}/{rules}: {error}"));
        assert!(cost > 0);

        if let Ok(optimal) = extract_mem(&green, root, &algorithm("maxsat.inc_scc")) {
            assert!(
                cost >= optimal,
                "bottom-up {cost} is below the optimum {optimal} on {seed}/{steps}/{rules}"
            );
        }
    }
}

#[test]
fn frontier_is_valid_on_tiny_pure_and_memory() {
    for (seed, steps) in [(1, 20), (2, 40), (3, 30)] {
        let (green, root, _) = pure::build(pure_params(seed, steps));
        let optimal = extract_mem(&green, root, &algorithm("maxsat.inc_scc"))
            .unwrap_or_else(|error| panic!("maxsat {seed}/{steps}: {error}"));
        let mut stats = ExtractStats::default();
        let cost = extract_mem_with_stats(
            &green,
            root,
            &algorithm("sat.descending.frontier"),
            Some(&mut stats),
        )
        .unwrap_or_else(|error| panic!("frontier {seed}/{steps}: {error}"));
        assert!(
            cost >= optimal,
            "frontier {cost} is below the optimum {optimal} on {seed}/{steps}"
        );
        assert_eq!(
            stats.custom["filtration.mode"],
            StatValue::Text("component_bottom_up_frontier".into())
        );
    }

    for (seed, steps, rules) in [(1, 20, "reorder"), (15, 20, "assoc_reorder")] {
        let (green, root, _) = generated::build(Params {
            seed,
            steps,
            weights: memory_weights(),
            optional_rules: rules_by_name(rules),
            addresses: generated::DEFAULT_ADDRESSES,
        });
        extract_mem(&green, root, &algorithm("sat.descending.frontier"))
            .unwrap_or_else(|error| panic!("frontier {seed}/{steps}/{rules}: {error}"));
    }
}

#[test]
fn dead_vertices_preserves_the_tiny_pure_cost() {
    for (seed, steps) in [(1, 20), (2, 40), (3, 30)] {
        let (green, root, _) = pure::build(pure_params(seed, steps));
        let optimal = extract_mem(&green, root, &algorithm("maxsat.inc_scc"))
            .unwrap_or_else(|error| panic!("maxsat {seed}/{steps}: {error}"));
        let filtered = extract_mem(&green, root, &algorithm("maxsat.dead"))
            .unwrap_or_else(|error| panic!("maxsat.dead {seed}/{steps}: {error}"));
        assert_eq!(filtered, optimal, "seed {seed}, steps {steps}");
    }
}
