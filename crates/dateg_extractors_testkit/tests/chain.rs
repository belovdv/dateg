//! End-to-end smoke test for the greedy chain solver.
//!
//! The generated memory programs have the expected shape (one `MInit` base and
//! stores consuming the previous version), so the chain path must be taken and
//! the result must be valid. The combinator entry still optimizes.

use std::time::Duration;

use dateg_extractors::dag::{Error, ExtractStats, StatValue};
use dateg_extractors_testkit::algorithms;
use dateg_extractors_testkit::indexes::mem::extract_mem_with_stats;
use dateg_extractors_testkit::sources::generated::{self, Params, memory_weights, rules_by_name};

fn build(
    seed: u64,
    steps: usize,
    rules: &'static str,
) -> (
    dateg_extractors_testkit::memory::Memory,
    dateg::TokenOpaque<dateg_extractors_testkit::memory::Mem>,
    usize,
) {
    generated::build(Params {
        seed,
        steps,
        weights: memory_weights(),
        optional_rules: rules_by_name(rules),
        addresses: generated::DEFAULT_ADDRESSES,
    })
}

fn algorithm(name: &str) -> algorithms::Algorithm {
    let mut algorithm = algorithms::by_name(name).unwrap().clone();
    algorithm.config.timeout = Some(Duration::from_secs(10));
    algorithm
}

#[test]
fn chain_solves_the_memory_shape_and_the_combinator_optimizes() {
    let (green, root, _) = build(1, 20, "reorder");

    let mut stats = ExtractStats::default();
    let cost = extract_mem_with_stats(&green, root, &algorithm("chain"), Some(&mut stats)).unwrap();
    assert_eq!(stats.custom["chain.shape"], StatValue::Text("ok".into()));
    assert!(matches!(
        stats.custom["chain.length"],
        StatValue::Count(length) if length > 0
    ));

    let optimal = extract_mem_with_stats(&green, root, &algorithm("maxsat.inc_scc"), None).unwrap();
    assert!(
        cost >= optimal,
        "chain returned {cost}, below the optimum {optimal}"
    );

    let mut stats = ExtractStats::default();
    let combined = extract_mem_with_stats(
        &green,
        root,
        &algorithm("combinator.thorough"),
        Some(&mut stats),
    )
    .unwrap();
    assert_eq!(combined, optimal);
    // `thorough` records each stage that ran; MaxSAT is the exact baseline.
    assert!(stats.custom.contains_key("combinator.baseline"));
}

#[test]
fn chain_solves_ac_and_write_reordering() {
    for (seed, steps) in [(15, 60), (6, 100), (17, 100), (23, 100)] {
        let (green, root, _) = build(seed, steps, "assoc_reorder");
        let mut stats = ExtractStats::default();
        let cost =
            extract_mem_with_stats(&green, root, &algorithm("chain"), Some(&mut stats)).unwrap();
        assert!(cost > 0);
        assert_eq!(stats.custom["chain.shape"], StatValue::Text("ok".into()));
    }
}

#[test]
fn chain_solves_longer_reordering_programs() {
    for (seed, steps) in [(15, 180), (6, 180), (5, 180), (17, 200)] {
        let (green, root, _) = build(seed, steps, "reorder");
        let mut stats = ExtractStats::default();
        let cost =
            extract_mem_with_stats(&green, root, &algorithm("chain"), Some(&mut stats)).unwrap();
        assert!(cost > 0);
        assert_eq!(stats.custom["chain.shape"], StatValue::Text("ok".into()));
    }
}

#[test]
fn chain_reports_unsolved_when_the_rollback_budget_runs_out() {
    let (green, root, _) = build(19, 100, "assoc_reorder");
    let mut stats = ExtractStats::default();
    let failure =
        extract_mem_with_stats(&green, root, &algorithm("chain"), Some(&mut stats)).unwrap_err();
    assert!(matches!(failure, Error::NotFound));
    assert_eq!(stats.custom["chain.shape"], StatValue::Text("ok".into()));
    assert_eq!(
        stats.custom["chain.result"],
        StatValue::Text("unsolved".into())
    );
}
