# DATEG

Alternative interface to [egglog](https://github.com/egraphs-good/egglog), built on top of `egglog-bridge`.

Status: ready to work (covers my use case), moderately tested, lacks some features (primarily containers)

Motivational feature: typed interface:
- simpler access to data (primitives)
- simpler extraction result traversing (enums)
- many invariants are checked at compile-time
- syntax highlighting and error messages from rust analyzer

Additional features:
- direct inserting/extracting data interface
- basic DAG extractor with support for quasi-linear sorts (requires z3)

Example: [crates/dateg_extractors/tests/dag_consume.rs](crates/dateg_extractors/tests/dag_consume.rs)
