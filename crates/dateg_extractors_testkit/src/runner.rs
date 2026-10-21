use std::collections::HashMap;

use rayon::prelude::*;

use super::algorithms::{self, Algorithm, Fit};
use super::case::{Case, Expected, Override, Purpose, SourceKind, Tier};

pub fn run_filtered(
    cases: Vec<Case>,
    tier: Option<Tier>,
    purpose: Option<Purpose>,
    source: Option<SourceKind>,
) {
    let cases: Vec<Case> = cases
        .into_iter()
        .filter(|case| tier.is_none_or(|tier| case.tier() == tier))
        .filter(|case| purpose.is_none_or(|purpose| case.purpose == purpose))
        .filter(|case| source.is_none_or(|source| source == case.source))
        .collect();

    let results: Vec<Result<Option<String>, String>> = cases
        .par_iter()
        .flat_map(|case| {
            let oracle_costs = oracle_costs(case);
            algorithms::all()
                .par_iter()
                .map(move |algorithm| check(case, algorithm, &oracle_costs))
        })
        .collect();

    let mut failures = vec![];
    let mut skips = vec![];
    for result in results {
        match result {
            Ok(Some(reason)) => skips.push(reason),
            Ok(None) => {}
            Err(error) => failures.push(error),
        }
    }

    if !failures.is_empty() {
        panic!(
            "{} matrix case(s) failed:\n{}",
            failures.len(),
            failures.join("\n")
        );
    }
    if !skips.is_empty() {
        eprintln!("{} case run(s) skipped:\n{}", skips.len(), skips.join("\n"));
    }
}

/// Runs each distinct oracle needed by this case once and caches its result.
/// Oracles are deliberately the slower, simpler implementations; caching avoids
/// repeating them for every algorithm/config under test.
fn oracle_costs(case: &Case) -> HashMap<&'static str, Result<usize, String>> {
    let mut needed = vec![];
    if matches!(case.expected, Expected::Unknown) {
        for algorithm in algorithms::all() {
            if algorithm.fit(case.complexity) == Fit::Infeasible {
                continue;
            }
            for name in algorithm.oracles {
                if !needed.contains(name) {
                    needed.push(*name);
                }
            }
        }
    }

    let mut results = HashMap::new();
    for name in needed {
        let Some(oracle) = algorithms::by_name(name) else {
            continue;
        };
        if oracle.fit(case.complexity) == Fit::Infeasible {
            continue;
        }
        let oracle = oracle.for_tier(case.tier());
        results.insert(name, (case.run)(&oracle).map_err(|error| error.to_string()));
    }
    results
}

fn check(
    case: &Case,
    algorithm: &Algorithm,
    oracle_costs: &HashMap<&'static str, Result<usize, String>>,
) -> Result<Option<String>, String> {
    let algorithm = &algorithm.for_tier(case.tier());
    for rule in case.overrides {
        match *rule {
            Override::Skip {
                algorithm: name,
                reason,
            } if name == algorithm.name => {
                return Ok(Some(format!(
                    "{} / {}: {reason}",
                    case.name, algorithm.name
                )));
            }
            Override::ExpectFailure {
                algorithm: name,
                reason,
            } if name == algorithm.name => {
                return check_failure(case, algorithm, reason).map(|()| None);
            }
            _ => {}
        }
    }

    if algorithm.fit(case.complexity) == Fit::Infeasible {
        return Ok(Some(format!(
            "{} / {}: infeasible at complexity {}",
            case.name, algorithm.name, case.complexity.size
        )));
    }

    match case.expected {
        Expected::Exact(expected) => check_exact(case, algorithm, expected).map(|()| None),
        Expected::Failure(reason) => check_failure(case, algorithm, reason).map(|()| None),
        Expected::Unknown => check_unknown(case, algorithm, oracle_costs),
    }
}

fn check_exact(case: &Case, algorithm: &Algorithm, expected: usize) -> Result<(), String> {
    match (case.run)(algorithm) {
        Ok(cost) if cost == expected => Ok(()),
        Ok(cost) => Err(format!(
            "{} / {}: expected cost {expected}, got {cost}",
            case.name, algorithm.name
        )),
        Err(error) => Err(format!(
            "{} / {}: expected success, got {error}",
            case.name, algorithm.name
        )),
    }
}

fn check_failure(case: &Case, algorithm: &Algorithm, reason: &str) -> Result<(), String> {
    match (case.run)(algorithm) {
        Err(error) if error.to_string().contains(reason) => Ok(()),
        Err(error) => Err(format!(
            "{} / {}: expected failure containing {reason:?}, got {:?}",
            case.name,
            algorithm.name,
            error.to_string()
        )),
        Ok(cost) => Err(format!(
            "{} / {}: expected failure containing {reason:?}, got cost {cost}",
            case.name, algorithm.name
        )),
    }
}

fn check_unknown(
    case: &Case,
    algorithm: &Algorithm,
    oracle_costs: &HashMap<&'static str, Result<usize, String>>,
) -> Result<Option<String>, String> {
    let Some((oracle, expected)) = algorithm
        .oracles
        .iter()
        .find_map(|name| oracle_costs.get(name).map(|result| (*name, result)))
    else {
        return run_unchecked(case, algorithm);
    };

    let expected = match expected {
        Ok(cost) => *cost,
        Err(error) => {
            return Ok(Some(format!(
                "{} / {}: oracle {oracle} failed: {error}",
                case.name, algorithm.name
            )));
        }
    };

    match (case.run)(algorithm) {
        Ok(cost) if cost == expected => Ok(None),
        Ok(cost) => Err(format!(
            "{} / {}: oracle {oracle} cost {expected}, got {cost}",
            case.name, algorithm.name
        )),
        Err(error) => Err(format!(
            "{} / {}: expected success, got {error}",
            case.name, algorithm.name
        )),
    }
}

fn run_unchecked(case: &Case, algorithm: &Algorithm) -> Result<Option<String>, String> {
    match (case.run)(algorithm) {
        Ok(_) => Ok(None),
        Err(error) => Err(format!(
            "{} / {}: expected successful extraction, got {error}",
            case.name, algorithm.name
        )),
    }
}
