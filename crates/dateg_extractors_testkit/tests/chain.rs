//! End-to-end smoke test for the greedy chain solver.
//!
//! The generated memory programs have the expected shape (one `MInit` base and
//! stores consuming the previous version), so the chain path must be taken and
//! the result must be valid. The combinator entry still optimizes.

use std::time::Duration;

use dateg_extractors::dag::{ExtractStats, StatValue};
use dateg_extractors_testkit::algorithms;
use dateg_extractors_testkit::indexes::mem::extract_mem_with_stats;
use dateg_extractors_testkit::sources::generated::{self, Params, memory_weights, rules_by_name};

fn build() -> (
    dateg_extractors_testkit::memory::Memory,
    dateg::TokenOpaque<dateg_extractors_testkit::memory::Mem>,
    usize,
) {
    generated::build(Params {
        seed: 1,
        steps: 20,
        weights: memory_weights(),
        optional_rules: rules_by_name("reorder"),
        addresses: generated::DEFAULT_ADDRESSES,
    })
}

fn algorithm(name: &str) -> algorithms::Algorithm {
    let mut algorithm = algorithms::by_name(name).unwrap().clone();
    algorithm.timeout = Duration::from_secs(10);
    algorithm
}

#[test]
fn chain_solves_the_memory_shape_and_the_combinator_optimizes() {
    let (green, root, _) = build();

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
        &algorithm("combinator.chain"),
        Some(&mut stats),
    )
    .unwrap();
    assert_eq!(combined, optimal);
    // The chain may already hit the optimum, in which case the tie keeps the
    // first stage; the second stage always runs and records its duration.
    assert!(stats.custom.contains_key("combinator.second"));
}
