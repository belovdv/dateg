use dateg_extractors_testkit::algorithms;
use dateg_extractors_testkit::indexes::mem::{MemIndex, linearize_int, linearize_mem};
use dateg_extractors_testkit::memory::{Memory, saturate};

fn extract(m: &Memory, root: dateg::TokenOpaque<dateg_extractors_testkit::memory::Int>) -> String {
    let algorithm = algorithms::by_name("maxsat.inc_scc").unwrap();
    let extraction = MemIndex::extract(
        m,
        root,
        (m.init, m.store),
        (
            m.cst, m.var, m.load, m.one, m.inc, m.add, m.sub, m.neg, m.mul, m.square,
        ),
        &algorithm.options(),
        None,
    )
    .unwrap();
    linearize_int(m, &extraction.index, root)
}

#[test]
fn linearize_forwarded_load() {
    let mut m = Memory::default();
    let init = m.add_init();
    let addr = m.add_cst(1);
    let value = m.add_cst(5);
    let stored = m.add_store(init, addr, value);
    let loaded = m.add_load(stored, addr);
    let root = m.add_add(loaded, value).canon(&m.eg);
    saturate(&mut m, &["alias"]);

    let text = extract(&m, root);
    assert_eq!(text, "%1 = cst 5\n%0 = add %1, %1\nroot = %0");
}

#[test]
fn linearize_store_only() {
    let mut m = Memory::default();
    let init = m.add_init();
    let addr = m.add_cst(1);
    let value = m.add_cst(5);
    let stored = m.add_store(init, addr, value).canon(&m.eg);
    saturate(&mut m, &["alias"]);

    let algorithm = algorithms::by_name("maxsat.inc_scc").unwrap();
    let extraction = MemIndex::extract(
        &m,
        stored,
        (m.init, m.store),
        (
            m.cst, m.var, m.load, m.one, m.inc, m.add, m.sub, m.neg, m.mul, m.square,
        ),
        &algorithm.options(),
        None,
    )
    .unwrap();
    let text = linearize_mem(&m, &extraction.index, stored);
    assert_eq!(
        text,
        "%1 = init\n%2 = cst 1\n%3 = cst 5\n%0 = store %1, %2, %3\nroot = %0"
    );
}
