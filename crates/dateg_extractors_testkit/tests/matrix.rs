use dateg_extractors_testkit::case::{Purpose, SourceKind, Tier};
use dateg_extractors_testkit::{runner, sources};

fn fast(source: SourceKind) {
    runner::run_filtered(
        sources::all(),
        Some(Tier::Fast),
        Some(Purpose::Verification),
        Some(source),
    );
}

#[test]
fn fast_handcrafted() {
    fast(SourceKind::Handcrafted);
}

#[test]
fn fast_special() {
    fast(SourceKind::Special);
}

#[test]
fn fast_random_sparse() {
    fast(SourceKind::RandomSparse);
}

#[test]
fn fast_random_dense() {
    fast(SourceKind::RandomDense);
}

#[test]
fn fast_consumption() {
    fast(SourceKind::Consumption);
}

#[test]
fn fast_random_memory() {
    fast(SourceKind::RandomMemory);
}

#[test]
fn fast_failures() {
    fast(SourceKind::Failure);
}

#[test]
fn fast_use_cases() {
    runner::run_filtered(
        sources::all(),
        Some(Tier::Fast),
        Some(Purpose::UseCase),
        Some(SourceKind::UseCase),
    );
}

#[test]
#[ignore = "tier: slow"]
fn slow_matrix() {
    runner::run_filtered(sources::all(), Some(Tier::Slow), None, None);
}

#[test]
#[ignore = "tier: soak"]
fn soak_matrix() {
    runner::run_filtered(sources::all(), Some(Tier::Soak), None, None);
}
