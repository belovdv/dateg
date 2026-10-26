use std::time::Duration;

use dateg_extractors::dag::ExtractOptions;

use crate::case::{Case, Expected, Purpose, SourceKind, Tier};
use crate::indexes::expr::{ExprIndex, extract_expr};
use crate::memory::{Memory, inc_chain};

macro_rules! case {
    ($name:literal, $reason:literal, $run:expr) => {{
        let expected = Expected::Failure($reason);
        Case::new(
            $name,
            Purpose::Verification,
            SourceKind::Failure,
            expected.complexity_hint(Tier::Fast),
            expected,
            $run,
        )
    }};
}

pub fn cases() -> Vec<Case> {
    vec![
        case!(
            "failure/missing_constructor",
            "root value is not present",
            |algorithm| {
                let mut m = Memory::default();
                let one = m.add_one();
                let two = m.add_inc(one);
                let root = m.add_square(two).canon(&m.eg);
                extract_expr(&m, root, algorithm)
            }
        ),
        case!("failure/timeout", "timed out", |algorithm| {
            let (m, root) = inc_chain(3);
            let mut config = algorithm.config.clone();
            config.timeout = Some(Duration::ZERO);
            let options = ExtractOptions::builder(config).build();
            ExprIndex::extract(&m, root, (m.one, m.inc, m.mul), &options, None)
                .map(|extraction| extraction.cost)
        }),
    ]
}
