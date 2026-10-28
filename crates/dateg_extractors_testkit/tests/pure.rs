//! Pure programs with several free inputs and one trailing store. These guard
//! the bench fixture shape and the best-effort fallback used for open rungs.

use std::time::Duration;

use dateg_extractors::dag::{
    AtomicAlgorithm, BestEffortConfig, ExtractStats, Node, SatDescendingConfig, SolverConfig,
    StatValue, TimeoutPolicy,
};
use dateg_extractors_testkit::algorithms;
use dateg_extractors_testkit::indexes::mem::{extract_int, extract_mem, extract_mem_with_stats};
use dateg_extractors_testkit::memory::Memory;
use dateg_extractors_testkit::sources::generated::{pure_weights, rules_by_name};
use dateg_extractors_testkit::sources::pure;

fn pure_params(seed: u64, steps: usize, vars: usize) -> pure::Params {
    pure::Params {
        seed,
        steps,
        vars,
        weights: pure_weights(),
        optional_rules: rules_by_name("assoc"),
    }
}

#[test]
fn variable_leaves_are_free() {
    let mut m = Memory::default();
    let table = m.var;
    let name = m.add_primitive_value("x".to_string());
    let var = m.row_add(table, (name,));

    let mut algorithm = algorithms::by_name("maxsat.inc_scc")
        .expect("maxsat")
        .clone();
    algorithm.config.timeout = Duration::from_secs(1);
    assert_eq!(extract_int(&m, var, &algorithm).expect("var"), 0);
}

#[test]
fn chain_handles_pure_programs_across_shapes() {
    let mut chain = algorithms::by_name("chain").expect("chain").clone();
    chain.config.timeout = Duration::from_secs(5);

    for (seed, vars) in [(1, 1), (2, 4), (3, 16)] {
        let (m, root, _) = pure::build(pure_params(seed, 20, vars));
        let cost = extract_mem(&m, root, &chain)
            .unwrap_or_else(|error| panic!("seed {seed}, vars {vars}: {error}"));
        assert!(cost >= 1);
    }

    for seed in 1..=8 {
        let (m, root, _) = pure::build(pure_params(seed, 20, 4));
        extract_mem(&m, root, &chain).unwrap_or_else(|error| panic!("seed {seed}: {error}"));
    }

    let (m, root, _) = pure::build(pure_params(38, 140, 4));
    assert!(extract_mem(&m, root, &chain).is_ok());
}

#[test]
fn soft_timeout_stops_after_a_solution() {
    let (m, root, _) = pure::build(pure_params(1, 20, 4));
    let config = SolverConfig::new(Node::bundle([
        Node::from(AtomicAlgorithm::Chain(BestEffortConfig::default())).with_label("first"),
        Node::from(AtomicAlgorithm::SatDescending(
            SatDescendingConfig::default(),
        ))
        .with_label("second"),
    ]))
    .with_timeout(Duration::from_secs(5))
    .with_soft_timeout(Duration::ZERO);

    let mut algorithm = algorithms::by_name("combinator.thorough")
        .expect("combinator.thorough")
        .clone();
    algorithm.config = config;
    algorithm.filtration = None;

    let mut stats = ExtractStats::default();
    let cost = extract_mem_with_stats(&m, root, &algorithm, Some(&mut stats))
        .expect("chain should find a solution before the soft deadline");
    assert!(cost >= 1);
    assert!(stats.custom.contains_key("combinator.first"));
    assert!(!stats.custom.contains_key("combinator.second"));
}

#[test]
fn combinator_returns_a_fallback_solution() {
    // A hard pure body: the maxsat stage cannot finish in the tiny budget, so
    // the combinator must rely on the best-effort baseline (or a second-stage
    // solution seeded from it) instead of failing.
    let (m, root, _) = pure::build(pure_params(38, 160, 4));
    let first = Node::from(AtomicAlgorithm::MaxSat(Default::default()))
        .with_timeout(TimeoutPolicy::Time(Duration::from_millis(20)))
        .with_label("first");
    let baseline =
        Node::from(AtomicAlgorithm::Chain(BestEffortConfig::default())).with_label("baseline");
    let second = Node::from(AtomicAlgorithm::SatDescending(
        SatDescendingConfig::default(),
    ))
    .with_timeout(TimeoutPolicy::Time(Duration::from_millis(20)))
    .with_label("second");
    let config = SolverConfig::new(Node::bundle([first.with_fallback(baseline), second]))
        .with_timeout(Duration::from_secs(2));

    let mut algorithm = algorithms::by_name("combinator.thorough")
        .expect("combinator.thorough")
        .clone();
    algorithm.config = config;
    algorithm.filtration = None;

    let mut stats = ExtractStats::default();
    let cost = extract_mem_with_stats(&m, root, &algorithm, Some(&mut stats))
        .expect("combinator fallback");
    assert!(cost >= 1);
    assert!(
        matches!(
            stats.custom.get("combinator.stage"),
            Some(StatValue::Text(stage))
                if stage == "baseline" || stage == "second" || stage == "first-fallback"
        ),
        "expected a fallback stage, got {:?}",
        stats.custom.get("combinator.stage")
    );
}
