//! Guards for the consumption dimension, which is easy to lose when a theory
//! or index is refactored: `store` must consume the memory token.

use std::time::Duration;

use dateg_extractors::dag::{ExtractStats, StatValue};
use dateg_extractors_testkit::algorithms;
use dateg_extractors_testkit::indexes::mem::extract_mem_with_stats;
use dateg_extractors_testkit::memory::Memory;

#[test]
fn store_consumes_memory_and_forks_are_counted() {
    let mut m = Memory::default();
    let one = m.add_one();
    let c1 = m.add_cst(1);
    let c2 = m.add_cst(2);

    let init = m.add_init();
    // Two stores consume the same memory version: a fork.
    let left = m.add_store(init, c1, one);
    let _right = m.add_store(init, c2, one);
    let next = m.add_store(left, c2, one);
    let root = m.add_store(next, c1, one).canon(&m.eg);

    let mut algorithm = algorithms::by_name("maxsat.inc_scc")
        .expect("maxsat")
        .clone();
    algorithm.config.timeout = Duration::from_secs(5);

    let mut stats = ExtractStats::default();
    let _ = extract_mem_with_stats(&m, root, &algorithm, Some(&mut stats));

    let count = |key: &str| match stats.custom.get(key) {
        Some(StatValue::Count(value)) => *value,
        other => panic!("{key} = {other:?}"),
    };
    assert_eq!(count("graph.consumed_values"), 3);
    assert_eq!(count("graph.consumption_edges"), 4);
    assert_eq!(count("graph.forked_values"), 1);
    assert_eq!(count("graph.max_consumers"), 2);
}
