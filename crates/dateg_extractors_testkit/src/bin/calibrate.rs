use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    time::{Duration, Instant},
};

use dateg_extractors::dag::Error;
use dateg_extractors_testkit::algorithms;
use dateg_extractors_testkit::indexes::mem::extract_mem;
use dateg_extractors_testkit::sources::generated::{
    Params, Weights, build, memory_weights, pure_weights,
};

enum Outcome {
    Solved { cost: usize, time: Duration },
    Timeout,
    Unsolved,
    Other,
    SaturationPanic,
}

fn try_steps(params: Params, timeout: Duration) -> Outcome {
    let built = catch_unwind(AssertUnwindSafe(|| build(params)));
    let Ok((m, root, _size)) = built else {
        return Outcome::SaturationPanic;
    };
    let mut algorithm = algorithms::by_name("maxsat.inc_scc")
        .expect("maxsat algorithm")
        .clone();
    algorithm.config.timeout = timeout;
    let started = Instant::now();
    match extract_mem(&m, root, &algorithm) {
        Ok(cost) => Outcome::Solved {
            cost,
            time: started.elapsed(),
        },
        Err(Error::Timeout) => Outcome::Timeout,
        Err(Error::NotFound | Error::Infeasible) => Outcome::Unsolved,
        Err(_) => Outcome::Other,
    }
}

fn main() {
    let start = env_usize("CALIBRATE_START", 4);
    let max = env_usize("CALIBRATE_MAX_STEPS", 4096);
    let increment = env_usize("CALIBRATE_STEP", 32).max(1);
    let extract_secs = env_u64("CALIBRATE_EXTRACT_SECS", 30);
    let timeout = Duration::from_secs(extract_secs);
    let seed = env_u64("CALIBRATE_SEED", 1);
    let only_profile = std::env::var("CALIBRATE_PROFILE").ok();

    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));

    let configs: &[(&str, Weights, &[&str])] = &[
        ("base", pure_weights(), &[]),
        ("assoc", pure_weights(), &["assoc"]),
        ("reorder", memory_weights(), &["store_reorder"]),
        (
            "assoc_reorder",
            memory_weights(),
            &["assoc", "store_reorder"],
        ),
    ];

    for &(name, weights, rules) in configs {
        if only_profile.as_deref().is_some_and(|only| only != name) {
            continue;
        }
        println!(
            "profile {name} seed {seed}: extraction timeout {extract_secs}s, step {increment}"
        );
        let mut last_success = None;
        let mut steps = start;
        while steps <= max {
            let params = Params {
                seed,
                steps,
                weights,
                optional_rules: rules,
                addresses: dateg_extractors_testkit::sources::generated::DEFAULT_ADDRESSES,
            };
            let started = Instant::now();
            let outcome = try_steps(params, timeout);
            let elapsed = started.elapsed();
            match outcome {
                Outcome::Solved { cost, time } => {
                    last_success = Some(steps);
                    println!(
                        "  steps={steps} solved cost={cost} extract={time:?} total={elapsed:?}"
                    );
                }
                Outcome::Timeout => {
                    println!(
                        "  steps={steps} TIMEOUT after {elapsed:?} (last success: {last_success:?})"
                    );
                    break;
                }
                Outcome::Unsolved => {
                    println!(
                        "  steps={steps} UNSOLVED after {elapsed:?} (last success: {last_success:?})"
                    );
                    break;
                }
                Outcome::Other => {
                    println!("  steps={steps} OTHER error after {elapsed:?}");
                    break;
                }
                Outcome::SaturationPanic => {
                    println!(
                        "  steps={steps} saturation failed (invalid profile point; last success: {last_success:?})"
                    );
                    break;
                }
            }
            steps += increment;
        }
    }

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
