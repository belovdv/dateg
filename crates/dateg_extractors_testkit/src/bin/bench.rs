//! Pass-rate benchmark: `passrate(algorithm, timeout)` over the graded set.
//!
//! ```text
//! cargo run --release --bin bench -- --timeouts 5,30,60,300
//! cargo run --release --bin bench -- --algorithm maxsat.inc_scc --timeouts 60
//! ```
//!
//! A case passes when extraction returns the expected optimal cost within the
//! timeout (validation is on). Build and extraction are reported separately;
//! build/saturation is not counted against the timeout.

use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    time::{Duration, Instant},
};

use dateg_extractors::dag::{ExtractError, ExtractStats};
use dateg_extractors_testkit::algorithms;
use dateg_extractors_testkit::bench::cases;
use dateg_extractors_testkit::indexes::mem::extract_mem_with_stats;
use dateg_extractors_testkit::sources::generated::build;

struct Args {
    algorithm: String,
    timeouts: Vec<u64>,
    filter: Vec<String>,
    csv: Option<std::path::PathBuf>,
}

fn parse_args() -> Args {
    let mut args = Args {
        algorithm: "maxsat.inc_scc".into(),
        timeouts: vec![5, 30, 60, 300],
        filter: vec![],
        csv: None,
    };
    let mut iter = std::env::args().skip(1);
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--algorithm" => args.algorithm = iter.next().expect("--algorithm value"),
            "--timeouts" => {
                args.timeouts = iter
                    .next()
                    .expect("--timeouts value")
                    .split(',')
                    .map(|value| value.trim().parse().expect("timeout seconds"))
                    .collect()
            }
            "--cases" => {
                args.filter = iter
                    .next()
                    .expect("--cases value")
                    .split(',')
                    .map(str::to_string)
                    .collect()
            }
            "--csv" => args.csv = Some(iter.next().expect("--csv value").into()),
            other => panic!("unknown argument {other:?}"),
        }
    }
    args.timeouts.sort_unstable();
    args.timeouts.dedup();
    args
}

fn main() {
    let args = parse_args();
    let algorithm = algorithms::by_name(&args.algorithm)
        .unwrap_or_else(|| panic!("unknown algorithm {:?}", args.algorithm))
        .clone();
    let mut selected: Vec<_> = cases()
        .into_iter()
        .filter(|case| {
            args.filter.is_empty() || args.filter.iter().any(|needle| case.name.contains(needle))
        })
        .collect();
    if selected.is_empty() {
        panic!("no cases matched {:?}", args.filter);
    }
    selected.sort_by(|left, right| left.reference_seconds.total_cmp(&right.reference_seconds));

    println!(
        "algorithm={} cases={} timeouts={:?}",
        algorithm.name,
        selected.len(),
        args.timeouts
    );
    println!(
        "{:<18} {:>8} {:>8} {:<9} {:>8} {:>8} {:>6}",
        "case", "build_s", "timeout", "status", "cost", "time_s", "pass"
    );

    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));

    let mut records = Vec::new();
    let mut pass_by_timeout = std::collections::BTreeMap::new();
    for timeout in args.timeouts.iter().copied() {
        pass_by_timeout.insert(timeout, 0usize);
    }

    for case in &selected {
        let build_started = Instant::now();
        let built = catch_unwind(AssertUnwindSafe(|| build(case.params)));
        let build_seconds = build_started.elapsed().as_secs_f64();
        let Ok((green, root, _)) = built else {
            println!(
                "{:<18} {:>8.2} {:>8} {:<9} {:>8} {:>8} {:>6}",
                case.name, build_seconds, "-", "saturation_failed", "-", "-", "FAIL"
            );
            continue;
        };

        for timeout in args.timeouts.iter().copied() {
            let mut run = algorithm.clone();
            run.timeout = Duration::from_secs(timeout);
            let mut stats = ExtractStats::default();
            let started = Instant::now();
            let outcome = catch_unwind(AssertUnwindSafe(|| {
                extract_mem_with_stats(&green, root, &run, Some(&mut stats))
            }));
            let seconds = started.elapsed().as_secs_f64();
            let (status, cost, passed) = match outcome {
                Ok(Ok(cost)) => match case.expected_cost {
                    Some(expected) if cost == expected => ("solved", Some(cost), true),
                    Some(_) => ("wrong_cost", Some(cost), false),
                    // No reference answer: a validated solution is the best we
                    // can check.
                    None => ("solved_unverified", Some(cost), true),
                },
                Ok(Err(ExtractError::Timeout)) => ("timeout", None, false),
                Ok(Err(ExtractError::Unsolved)) => ("unsolved", None, false),
                Ok(Err(_)) => ("error", None, false),
                Err(_) => ("panic", None, false),
            };
            if passed {
                *pass_by_timeout.get_mut(&timeout).expect("timeout key") += 1;
            }
            let cost_text = cost.map(|cost| cost.to_string()).unwrap_or_default();
            println!(
                "{:<18} {:>8.2} {:>7}s {:<16} {:>8} {:>10.3} {:>6}",
                case.name,
                build_seconds,
                timeout,
                status,
                cost_text,
                seconds,
                if passed { "PASS" } else { "FAIL" }
            );
            records.push((
                case.name.to_string(),
                algorithm.name.to_string(),
                timeout,
                status.to_string(),
                cost,
                case.expected_cost,
                case.reference_seconds,
                case.reference_exact,
                seconds,
                build_seconds,
                passed,
            ));
        }
    }

    std::panic::set_hook(previous_hook);

    if let Some(path) = &args.csv {
        let mut text = String::from(
            "case,algorithm,timeout_s,status,cost,expected,reference_s,reference_exact,time_s,build_s,pass\n",
        );
        for (
            name,
            algorithm,
            timeout,
            status,
            cost,
            expected,
            reference,
            reference_exact,
            seconds,
            build,
            passed,
        ) in &records
        {
            text.push_str(&format!(
                "{name},{algorithm},{timeout},{status},{},{},{reference:.2},{reference_exact},{seconds:.6},{build:.6},{}\n",
                cost.map(|cost| cost.to_string()).unwrap_or_default(),
                expected
                    .map(|expected| expected.to_string())
                    .unwrap_or_default(),
                if *passed { "pass" } else { "fail" }
            ));
        }
        std::fs::write(path, text).expect("write csv");
        println!("wrote {}", path.display());
    }

    println!();
    let total = selected.len();
    for (timeout, passed) in pass_by_timeout {
        let percent = 100.0 * passed as f64 / total as f64;
        println!(
            "passrate {} at timeout {:>5}s: {}/{} ({:.1}%)",
            algorithm.name, timeout, passed, total, percent
        );
    }
    if selected.iter().any(|case| case.expected_cost.is_none()) {
        println!(
            "* {} top-rung case(s) have no reference cost: a valid solution counts,\n  optimality is not checked.",
            selected
                .iter()
                .filter(|case| case.expected_cost.is_none())
                .count()
        );
    }
}
