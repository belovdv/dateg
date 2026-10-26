use std::time::Instant;

use super::*;
use crate::dag::{Error, check_deadline};

pub fn solve(
    g: &Graph,
    roots: &[VertexId],
    view: &View,
    deadline: Option<Instant>,
    soft: Option<Instant>,
) -> Result<(usize, Vec<bool>), Error> {
    let n = g.graph.node_count();
    assert!(n < 64, "naive solver supports at most 63 nodes, got {n}");
    let domain = reachable(g, roots, view);

    let mut best_cost = usize::MAX;
    let mut best_assignment = vec![false; n];
    let max_mask = 1u64 << n;

    // Masks are visited in node-id order, so equal-cost candidates are resolved
    // deterministically by node order.
    for mask in 0..max_mask {
        if mask % 1024 == 0 {
            check_deadline(deadline)?;
            if soft.is_some_and(|soft| Instant::now() >= soft) {
                return if best_cost == usize::MAX {
                    Err(Error::Timeout)
                } else {
                    Ok((best_cost, best_assignment))
                };
            }
        }

        // Only root-reachable, enabled vertices are solver variables. Leaving
        // the other bits free would let a mask "select" a disabled vertex and
        // then leak it into the collected index.
        let assignment: Vec<bool> = (0..n).map(|i| domain[i] && (mask >> i) & 1 != 0).collect();
        if !check_constraints(g, roots, view, &assignment) {
            continue;
        }

        let cost = assignment_cost(g, view, &assignment);
        if cost < best_cost {
            best_cost = cost;
            best_assignment = assignment;
        }
    }

    if best_cost == usize::MAX {
        Err(Error::Infeasible)
    } else {
        Ok((best_cost, best_assignment))
    }
}
