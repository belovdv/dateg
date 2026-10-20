use std::time::Instant;

use super::*;
use crate::dag::{ExtractError, check_deadline};

pub fn solve(g: &Graph, deadline: Option<Instant>) -> Result<(usize, Vec<bool>), ExtractError> {
    let n = g.graph.node_count();
    assert!(n < 64, "naive solver supports at most 63 nodes, got {n}");

    let mut best_cost = usize::MAX;
    let mut best_assignment = vec![false; n];
    let max_mask = 1u64 << n;

    for mask in 0..max_mask {
        if mask % 1024 == 0 {
            check_deadline(deadline)?;
        }

        let assignment: Vec<bool> = (0..n).map(|i| (mask >> i) & 1 != 0).collect();
        if !g.check_constraints(&assignment) {
            continue;
        }

        let cost: usize = (0..n)
            .filter(|&i| g.used[i] && assignment[i])
            .map(|i| g.graph[NodeIndex::new(i)].cost)
            .sum();

        if cost < best_cost {
            best_cost = cost;
            best_assignment = assignment;
        }
    }

    if best_cost == usize::MAX {
        Err(ExtractError::Unsolvable)
    } else {
        Ok((best_cost, best_assignment))
    }
}
