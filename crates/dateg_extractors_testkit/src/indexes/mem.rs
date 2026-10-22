#![allow(clippy::enum_variant_names)]

use std::collections::{HashMap, HashSet};

use dateg::TokenOpaque;
use dateg_extractors::dag::{ExtractError, index_dag};

use crate::algorithms::Algorithm;
use crate::memory::{Int, Mem, Memory};

index_dag!(MemIndex
    mem: MemEnum (datatype Mem
        MInit ()
        MStore (Mem Int Int) { |_, _| Some(1) } { |(mem, ..)| (mem,) }
    )
    int: IntEnum (datatype Int
        MCst (usize)
        MVar (String)
        MLoad (Mem Int)
        MOne ()
        MInc (Int)
        MAdd (Int Int)
        MSub (Int Int)
        MNeg (Int)
        MMul (Int Int)
        MSquare (Int)
    )
);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Node {
    Mem(TokenOpaque<Mem>),
    Int(TokenOpaque<Int>),
}

fn validate_node(
    index: &MemIndex,
    node: Node,
    visiting: &mut HashSet<Node>,
    done: &mut HashSet<Node>,
) -> usize {
    if done.contains(&node) {
        return 0;
    }
    assert!(!visiting.contains(&node), "index cycle at {node:?}");
    visiting.insert(node);
    let mut total = 1;
    match node {
        Node::Mem(token) => {
            let value = index
                .mem
                .get(&token)
                .unwrap_or_else(|| panic!("missing mem entry for {token:?}"));
            match value {
                MemEnum::MInit() => {}
                MemEnum::MStore(mem, addr, val) => {
                    total += validate_node(index, Node::Mem(*mem), visiting, done);
                    total += validate_node(index, Node::Int(*addr), visiting, done);
                    total += validate_node(index, Node::Int(*val), visiting, done);
                }
            }
        }
        Node::Int(token) => {
            let value = index
                .int
                .get(&token)
                .unwrap_or_else(|| panic!("missing int entry for {token:?}"));
            match value {
                IntEnum::MCst(_) | IntEnum::MVar(_) | IntEnum::MOne() => {}
                IntEnum::MLoad(mem, addr) => {
                    total += validate_node(index, Node::Mem(*mem), visiting, done);
                    total += validate_node(index, Node::Int(*addr), visiting, done);
                }
                IntEnum::MInc(a) | IntEnum::MNeg(a) | IntEnum::MSquare(a) => {
                    total += validate_node(index, Node::Int(*a), visiting, done);
                }
                IntEnum::MAdd(a, b) | IntEnum::MSub(a, b) | IntEnum::MMul(a, b) => {
                    total += validate_node(index, Node::Int(*a), visiting, done);
                    total += validate_node(index, Node::Int(*b), visiting, done);
                }
            }
        }
    }
    visiting.remove(&node);
    done.insert(node);
    total
}

fn extract(
    m: &Memory,
    root: impl dateg::TokenOpaqueMarker,
    node: Node,
    algorithm: &Algorithm,
    stats: Option<&mut dateg_extractors::dag::ExtractStats>,
) -> Result<usize, ExtractError> {
    let extraction = MemIndex::extract(
        m,
        root,
        (m.init, m.store),
        (
            m.cst, m.var, m.load, m.one, m.inc, m.add, m.sub, m.neg, m.mul, m.square,
        ),
        &algorithm.options(),
        stats,
    )?;
    let cost = validate_node(
        &extraction.index,
        node,
        &mut HashSet::new(),
        &mut HashSet::new(),
    );
    assert_eq!(cost, extraction.cost);
    Ok(extraction.cost)
}

pub fn linearize(m: &Memory, index: &MemIndex, root: Node) -> String {
    fn visit(
        m: &Memory,
        index: &MemIndex,
        node: Node,
        names: &mut HashMap<Node, usize>,
        lines: &mut Vec<String>,
        visiting: &mut HashSet<Node>,
        done: &mut HashSet<Node>,
    ) {
        if done.contains(&node) {
            return;
        }
        assert!(!visiting.contains(&node), "index cycle at {node:?}");
        let id = names.len();
        names.insert(node, id);
        visiting.insert(node);
        let line = match node {
            Node::Mem(token) => match index.mem.get(&token).expect("validated mem value") {
                MemEnum::MInit() => format!("%{id} = init"),
                MemEnum::MStore(mem, addr, value) => {
                    visit(m, index, Node::Mem(*mem), names, lines, visiting, done);
                    visit(m, index, Node::Int(*addr), names, lines, visiting, done);
                    visit(m, index, Node::Int(*value), names, lines, visiting, done);
                    format!(
                        "%{id} = store %{}, %{}, %{}",
                        names[&Node::Mem(*mem)],
                        names[&Node::Int(*addr)],
                        names[&Node::Int(*value)]
                    )
                }
            },
            Node::Int(token) => match index.int.get(&token).expect("validated int value") {
                IntEnum::MCst(value) => format!("%{id} = cst {}", value.get(&m.eg)),
                IntEnum::MVar(value) => format!("%{id} = var {}", value.get(&m.eg)),
                IntEnum::MOne() => format!("%{id} = one"),
                IntEnum::MLoad(mem, addr) => {
                    visit(m, index, Node::Mem(*mem), names, lines, visiting, done);
                    visit(m, index, Node::Int(*addr), names, lines, visiting, done);
                    format!(
                        "%{id} = load %{}, %{}",
                        names[&Node::Mem(*mem)],
                        names[&Node::Int(*addr)]
                    )
                }
                IntEnum::MInc(a) => {
                    visit(m, index, Node::Int(*a), names, lines, visiting, done);
                    format!("%{id} = inc %{}", names[&Node::Int(*a)])
                }
                IntEnum::MNeg(a) => {
                    visit(m, index, Node::Int(*a), names, lines, visiting, done);
                    format!("%{id} = neg %{}", names[&Node::Int(*a)])
                }
                IntEnum::MSquare(a) => {
                    visit(m, index, Node::Int(*a), names, lines, visiting, done);
                    format!("%{id} = square %{}", names[&Node::Int(*a)])
                }
                IntEnum::MAdd(a, b) => {
                    visit(m, index, Node::Int(*a), names, lines, visiting, done);
                    visit(m, index, Node::Int(*b), names, lines, visiting, done);
                    format!(
                        "%{id} = add %{}, %{}",
                        names[&Node::Int(*a)],
                        names[&Node::Int(*b)]
                    )
                }
                IntEnum::MSub(a, b) => {
                    visit(m, index, Node::Int(*a), names, lines, visiting, done);
                    visit(m, index, Node::Int(*b), names, lines, visiting, done);
                    format!(
                        "%{id} = sub %{}, %{}",
                        names[&Node::Int(*a)],
                        names[&Node::Int(*b)]
                    )
                }
                IntEnum::MMul(a, b) => {
                    visit(m, index, Node::Int(*a), names, lines, visiting, done);
                    visit(m, index, Node::Int(*b), names, lines, visiting, done);
                    format!(
                        "%{id} = mul %{}, %{}",
                        names[&Node::Int(*a)],
                        names[&Node::Int(*b)]
                    )
                }
            },
        };
        visiting.remove(&node);
        done.insert(node);
        lines.push(line);
    }

    let mut names = HashMap::new();
    let mut lines = vec![];
    visit(
        m,
        index,
        root,
        &mut names,
        &mut lines,
        &mut HashSet::new(),
        &mut HashSet::new(),
    );
    let root_name = names[&root];
    lines.push(format!("root = %{root_name}"));
    lines.join("\n")
}

pub fn linearize_int(m: &Memory, index: &MemIndex, root: TokenOpaque<Int>) -> String {
    linearize(m, index, Node::Int(root))
}

pub fn linearize_mem(m: &Memory, index: &MemIndex, root: TokenOpaque<Mem>) -> String {
    linearize(m, index, Node::Mem(root))
}

pub fn extract_mem(
    m: &Memory,
    root: TokenOpaque<Mem>,
    algorithm: &Algorithm,
) -> Result<usize, ExtractError> {
    extract(m, root, Node::Mem(root), algorithm, None)
}

pub fn extract_mem_with_stats(
    m: &Memory,
    root: TokenOpaque<Mem>,
    algorithm: &Algorithm,
    stats: Option<&mut dateg_extractors::dag::ExtractStats>,
) -> Result<usize, ExtractError> {
    extract(m, root, Node::Mem(root), algorithm, stats)
}

pub fn extract_int(
    m: &Memory,
    root: TokenOpaque<Int>,
    algorithm: &Algorithm,
) -> Result<usize, ExtractError> {
    extract(m, root, Node::Int(root), algorithm, None)
}
