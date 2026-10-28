//! ComponentOptimal filtration is validated against the current master-best solver,
//! not against `naive`. Tiny cases keep the master reference cheap; the slow
//! tier covers the pure rungs where the unfiltered solver is too slow for the
//! fast matrix.

use std::time::Duration;

use dateg_extractors_testkit::algorithms::{self, Algorithm};
use dateg_extractors_testkit::indexes::mem::extract_mem;
use dateg_extractors_testkit::sources::generated::{pure_weights, rules_by_name};
use dateg_extractors_testkit::sources::pure;

fn params(seed: u64, steps: usize) -> pure::Params {
    pure::Params {
        seed,
        steps,
        vars: 4,
        weights: pure_weights(),
        optional_rules: rules_by_name("assoc"),
    }
}

fn with_timeout(name: &str, seconds: u64) -> Algorithm {
    let mut algorithm = algorithms::by_name(name)
        .unwrap_or_else(|| panic!("missing algorithm {name}"))
        .clone();
    algorithm.config.timeout = Duration::from_secs(seconds);
    algorithm
}

#[test]
fn component_filter_matches_master_on_tiny_pure() {
    let master = with_timeout("maxsat.inc_scc", 10);
    let filtered = with_timeout("combinator.component", 10);

    for (seed, steps) in [(1, 20), (2, 40), (3, 30)] {
        let (m, root, _) = pure::build(params(seed, steps));
        let expected =
            extract_mem(&m, root, &master).unwrap_or_else(|e| panic!("master {seed}/{steps}: {e}"));
        let actual = extract_mem(&m, root, &filtered)
            .unwrap_or_else(|e| panic!("filtered {seed}/{steps}: {e}"));
        assert_eq!(actual, expected, "seed {seed}, steps {steps}");
    }
}

#[test]
#[ignore = "tier: slow"]
fn component_filter_matches_master_on_larger_pure() {
    let master = with_timeout("maxsat.inc_scc", 60);
    let filtered = with_timeout("combinator.component", 60);

    for (seed, steps) in [(9, 100), (38, 160), (21, 200)] {
        let (m, root, _) = pure::build(params(seed, steps));
        let expected =
            extract_mem(&m, root, &master).unwrap_or_else(|e| panic!("master {seed}/{steps}: {e}"));
        let actual = extract_mem(&m, root, &filtered)
            .unwrap_or_else(|e| panic!("filtered {seed}/{steps}: {e}"));
        assert_eq!(actual, expected, "seed {seed}, steps {steps}");
    }
}
