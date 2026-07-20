use super::*;

pub fn solve(g: &Graph) -> (usize, Vec<bool>) {
    let n = g.graph.node_count();
    let mut best_cost = usize::MAX;
    let mut best_assignment = vec![false; n];
    assert!(n < 64);
    let max_mask = 1u64 << n;

    for mask in 0..max_mask {
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

    (best_cost, best_assignment)
}
