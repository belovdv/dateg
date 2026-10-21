use dateg::TokenOpaque;

use crate::case::{Case, Complexity, Expected, Purpose, SourceKind, Tier};
use crate::indexes::mem::extract_int;
use crate::memory::{Int, Memory, saturate};

fn fixture() -> (Memory, TokenOpaque<Int>, usize) {
    let mut m = Memory::default();
    let one = m.add_one();
    let a = m.add_add(one, one);
    let b = m.add_mul(a, one);
    let c = m.add_add(a, b);
    let d = m.add_add(c, one);
    saturate(&mut m, &["tree", "ac"]);
    let root = d.canon(&m.eg);
    let size = m.term_count();
    (m, root, size)
}

/// Reassociation is a real use case, not a hand-written corner case. The
/// optimal cost is not precomputed; when no oracle is feasible the
/// case runs as a benchmark and only checks extraction validity.
pub fn cases() -> Vec<Case> {
    let (_, _, size) = fixture();
    vec![Case::new(
        "use_case/ac_reassociation",
        Purpose::UseCase,
        SourceKind::UseCase,
        Complexity::new(Tier::Fast, size),
        Expected::Unknown,
        |algorithm| {
            let (m, root, _) = fixture();
            extract_int(&m, root, algorithm)
        },
    )]
}
