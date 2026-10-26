//! One-point probe for calibration: build a generated program, print egraph
//! size, graph-shape metrics and (optionally) the extraction outcome.
//!
//! Env:
//! - `PROBE_SEED` (default 1)
//! - `PROBE_STEPS` (default 4)
//! - `PROBE_SHAPE` (`memory` | `pure`, default `memory`)
//! - `PROBE_PROFILE` (`pure` | `memory`, default `memory`)
//! - `PROBE_WEIGHTS` comma-separated weights overriding the profile
//! - `PROBE_RULES` (`none` | `assoc` | `reorder` | `assoc_reorder`, default `reorder`)
//! - `PROBE_ADDRESSES` number of constant addresses (default 8)
//! - `PROBE_VARS` free variables in a pure shape (default 4)
//! - `PROBE_TIMEOUT` extraction seconds (default 10)
//! - `PROBE_ALGORITHM` solver name (default `maxsat.inc_scc`)
//! - `PROBE_CYCLE_BLOCKING` (`scc` | `shortest`) for either solver
//! - `PROBE_PREEMPTIVE` (`1` | `0`) for preemptive 4-cycle blocking
//! - `PROBE_METRICS_ONLY` when set, stop right after graph construction
//!
//! Output is a single `key=value` line prefixed with `RESULT `.

use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    time::{Duration, Instant},
};

use dateg_extractors::dag::{AtomicAlgorithm, CycleBlocking, Error, ExtractStats, Node, StatValue};
use dateg_extractors_testkit::algorithms;
use dateg_extractors_testkit::indexes::mem::{CostModel, extract_mem_with_stats, set_cost_model};
use dateg_extractors_testkit::sources::generated::{self, Params};
use dateg_extractors_testkit::sources::pure;

fn main() {
    if std::env::var("PROBE_COST_MODEL").as_deref() == Ok("weighted") {
        set_cost_model(CostModel::Weighted);
    }
    let seed = env_u64("PROBE_SEED", 1);
    let steps = env_usize("PROBE_STEPS", 4);
    let profile = std::env::var("PROBE_PROFILE").unwrap_or_else(|_| "memory".into());
    let shape = std::env::var("PROBE_SHAPE").unwrap_or_else(|_| "memory".into());
    let vars = env_usize("PROBE_VARS", 4);
    let weights = match std::env::var("PROBE_WEIGHTS") {
        Ok(spec) => generated::parse_weights(&spec),
        Err(_) => generated::weights_by_name(&profile)
            .unwrap_or_else(|| panic!("unknown profile {profile:?}")),
    };
    let rules_name = std::env::var("PROBE_RULES").unwrap_or_else(|_| "reorder".into());
    let rules = generated::rules_by_name(&rules_name);
    let timeout_secs = env_u64("PROBE_TIMEOUT", 10);
    let addresses = env_usize(
        "PROBE_ADDRESSES",
        dateg_extractors_testkit::sources::generated::DEFAULT_ADDRESSES,
    );
    let metrics_only = std::env::var("PROBE_METRICS_ONLY").is_ok();

    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));

    let params = Params {
        seed,
        steps,
        weights,
        optional_rules: rules,
        addresses,
    };
    let build_started = Instant::now();
    let built = catch_unwind(AssertUnwindSafe(|| match shape.as_str() {
        "memory" => generated::build(params),
        "pure" => pure::build(pure::Params {
            seed,
            steps,
            vars,
            weights,
            optional_rules: rules,
        }),
        other => panic!("unknown PROBE_SHAPE {other:?}"),
    }));
    let build_time = build_started.elapsed();

    let mut fields = vec![
        format!("seed={seed}"),
        format!("steps={steps}"),
        format!("shape={shape}"),
        format!("profile={profile}"),
        format!("rules={rules_name}"),
        format!("weights={weights}"),
        format!("addresses={addresses}"),
        format!("vars={vars}"),
        format!("build_s={:.6}", build_time.as_secs_f64()),
    ];
    let Ok((m, root, size)) = built else {
        fields.push("status=saturation_failed".into());
        println!("RESULT {}", fields.join(" "));
        std::panic::set_hook(previous_hook);
        return;
    };
    fields.push(format!("size={size}"));

    let mut algorithm = algorithms::by_name(
        &std::env::var("PROBE_ALGORITHM").unwrap_or_else(|_| "maxsat.inc_scc".into()),
    )
    .expect("algorithm")
    .clone();
    let first_budget = std::env::var("PROBE_FIRST_BUDGET_MS")
        .ok()
        .map(|value| Duration::from_millis(value.parse().expect("PROBE_FIRST_BUDGET_MS")));
    let first_share = std::env::var("PROBE_FIRST_SHARE")
        .ok()
        .map(|value| value.parse().expect("PROBE_FIRST_SHARE"));
    if first_budget.is_some() || first_share.is_some() {
        if let Some(config) =
            algorithms::with_legacy_budgets(&algorithm.name, first_share, first_budget, None)
        {
            algorithm.config = config;
        }
    }
    algorithm.config.timeout = Some(if metrics_only {
        Duration::ZERO
    } else {
        Duration::from_secs(timeout_secs)
    });
    match &mut algorithm.config.pipeline {
        Node::Atomic(AtomicAlgorithm::SatDescending(config)) => {
            if let Ok(value) = std::env::var("PROBE_PREEMPTIVE") {
                config.preemptive_4cycles = parse_bool(&value);
            }
            if let Ok(value) = std::env::var("PROBE_CYCLE_BLOCKING") {
                config.cycle_blocking = parse_cycle_blocking(&value);
            }
        }
        Node::Atomic(AtomicAlgorithm::MaxSat(config)) => {
            if let Ok(value) = std::env::var("PROBE_PREEMPTIVE") {
                config.preemptive_4cycles = parse_bool(&value);
            }
            if let Ok(value) = std::env::var("PROBE_CYCLE_BLOCKING") {
                config.cycle_blocking = parse_cycle_blocking(&value);
            }
        }
        _ => {}
    }

    let mut stats = ExtractStats::default();
    let started = Instant::now();
    let result = extract_mem_with_stats(&m, root, &algorithm, Some(&mut stats));
    let extract_time = started.elapsed();

    let status = match &result {
        Ok(_) => "solved",
        Err(Error::Timeout) => "timeout",
        Err(Error::NotFound) => "unsolved",
        Err(Error::Infeasible) => "unsolvable",
        Err(_) => "other",
    };
    fields.push(format!("status={status}"));
    fields.push(format!("extract_s={:.6}", extract_time.as_secs_f64()));
    if let Ok(cost) = result {
        fields.push(format!("cost={cost}"));
    }
    for (key, value) in &stats.custom {
        let rendered = match value {
            StatValue::Count(value) => value.to_string(),
            StatValue::Duration(value) => format!("{:.6}", value.as_secs_f64()),
            StatValue::Text(value) => value.clone(),
        };
        let rendered = rendered.replace(' ', "_");
        fields.push(format!("{key}={rendered}"));
    }
    println!("RESULT {}", fields.join(" "));

    std::panic::set_hook(previous_hook);
}

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn env_u64(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn parse_bool(value: &str) -> bool {
    match value {
        "1" | "true" => true,
        "0" | "false" => false,
        other => panic!("expected boolean, got {other:?}"),
    }
}

fn parse_cycle_blocking(value: &str) -> CycleBlocking {
    match value {
        "scc" => CycleBlocking::Scc,
        "shortest" => CycleBlocking::Shortest,
        other => panic!("unknown PROBE_CYCLE_BLOCKING {other:?}"),
    }
}
