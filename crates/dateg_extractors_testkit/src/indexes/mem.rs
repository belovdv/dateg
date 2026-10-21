#![allow(clippy::enum_variant_names)]

use std::collections::HashSet;

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
) -> Result<usize, ExtractError> {
    let extraction = MemIndex::extract(
        m,
        root,
        (m.init, m.store),
        (
            m.cst, m.var, m.load, m.one, m.inc, m.add, m.sub, m.neg, m.mul, m.square,
        ),
        &algorithm.options(),
        None,
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

pub fn extract_mem(
    m: &Memory,
    root: TokenOpaque<Mem>,
    algorithm: &Algorithm,
) -> Result<usize, ExtractError> {
    extract(m, root, Node::Mem(root), algorithm)
}

pub fn extract_int(
    m: &Memory,
    root: TokenOpaque<Int>,
    algorithm: &Algorithm,
) -> Result<usize, ExtractError> {
    extract(m, root, Node::Int(root), algorithm)
}
