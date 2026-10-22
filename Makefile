.PHONY: test stress soak test-all hand special random-sparse random-dense random-memory consumption use-cases failures calibrate bench probe check

test:
	cargo test --workspace

RAYON_NUM_THREADS ?= 12

stress:
	RAYON_NUM_THREADS=$(RAYON_NUM_THREADS) cargo test --release -p dateg_extractors_testkit --test matrix --no-run
	RAYON_NUM_THREADS=$(RAYON_NUM_THREADS) timeout -k 5 300 cargo test --release -p dateg_extractors_testkit --test matrix -- --ignored slow_

soak:
	RAYON_NUM_THREADS=$(RAYON_NUM_THREADS) cargo test --release -p dateg_extractors_testkit --test matrix --no-run
	RAYON_NUM_THREADS=$(RAYON_NUM_THREADS) timeout -k 5 900 cargo test --release -p dateg_extractors_testkit --test matrix -- --ignored soak_

test-all:
	cargo test --workspace -- --include-ignored

hand:
	cargo test -p dateg_extractors_testkit --test matrix fast_handcrafted

special:
	cargo test -p dateg_extractors_testkit --test matrix fast_special

random-sparse:
	cargo test -p dateg_extractors_testkit --test matrix fast_random_sparse

random-dense:
	cargo test -p dateg_extractors_testkit --test matrix fast_random_dense

random-memory:
	cargo test -p dateg_extractors_testkit --test matrix fast_random_memory

consumption:
	cargo test -p dateg_extractors_testkit --test matrix fast_consumption

use-cases:
	cargo test -p dateg_extractors_testkit --test matrix fast_use_cases

failures:
	cargo test -p dateg_extractors_testkit --test matrix fast_failures

calibrate:
	timeout -k 5 1800 cargo run --release -p dateg_extractors_testkit --bin calibrate

# Pass rate of a solver over the graded benchmark set.
# Usage: make bench                       (5/30/60/300 s timeouts)
#        make bench BENCH_ARGS="--algorithm maxsat.inc_scc --timeouts 60"
BENCH_ARGS ?=
bench:
	cargo run --release -p dateg_extractors_testkit --bin bench -- $(BENCH_ARGS)

# One calibration point: make probe PROBE_SEED=6 PROBE_STEPS=134
PROBE_SEED ?= 6
PROBE_STEPS ?= 134
PROBE_RULES ?= reorder
PROBE_TIMEOUT ?= 10
probe:
	PROBE_SEED=$(PROBE_SEED) PROBE_STEPS=$(PROBE_STEPS) PROBE_RULES=$(PROBE_RULES) \
		PROBE_TIMEOUT=$(PROBE_TIMEOUT) \
		cargo run --release -p dateg_extractors_testkit --bin probe

check:
	cargo check --workspace --all-targets
