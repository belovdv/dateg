.PHONY: test stress soak test-all hand special random-sparse random-dense random-memory consumption use-cases failures check

test:
	cargo test --workspace

RAYON_NUM_THREADS ?= 12

stress:
	RAYON_NUM_THREADS=$(RAYON_NUM_THREADS) cargo test --release -p dateg_extractors_testkit --test matrix --no-run
	RAYON_NUM_THREADS=$(RAYON_NUM_THREADS) timeout -k 5 300 cargo test --release -p dateg_extractors_testkit --test matrix -- --ignored slow_

soak:
	RAYON_NUM_THREADS=$(RAYON_NUM_THREADS) cargo test --release -p dateg_extractors_testkit --test matrix --no-run
	RAYON_NUM_THREADS=$(RAYON_NUM_THREADS) timeout -k 5 300 cargo test --release -p dateg_extractors_testkit --test matrix -- --ignored soak_

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

check:
	cargo check --workspace --all-targets
