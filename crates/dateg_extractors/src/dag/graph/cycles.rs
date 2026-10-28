//! Bounded enumeration of class cycles for preemptive acyclicity clauses.
//!
//! A class cycle is a cycle of e-classes in the quotient graph: an edge
//! `c -> c'` exists when a constructor of `c` has `c'` as a child. One clause
//! per class cycle rules out every constructor combination that realizes it,
//! so the length bound covers many node cycles without enumerating them.

use std::{collections::BTreeMap, time::Instant};

use super::interface::{Graph, VertexId};

/// Ceiling for the derived length when `max_length` is `None`. The relative
/// budgets normally bind earlier; 6 is where the measured first-model time
/// stopped improving and prep started to grow.
pub(crate) const AUTO_MAX_LENGTH: usize = 6;

/// What the enumeration did. `truncated` marks a cap or deadline that stopped
/// it before the requested lengths were exhausted; `reached` is the last length
/// it started.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ClassCycleEnumeration {
    pub(crate) cycles: usize,
    pub(crate) literals: usize,
    pub(crate) visited: usize,
    pub(crate) ceiling: usize,
    pub(crate) reached: usize,
    pub(crate) truncated: bool,
}

/// Call `emit(classes, members)` for elementary class cycles up to
/// `max_length`, shortest length first. `members[i]` are the constructors of
/// `classes[i]` with `classes[(i + 1) % len]` as a child.
///
/// Cycles are emitted with their smallest class id first, so each directed
/// cycle appears once. Caps stop before the next emission; the deadline is
/// checked during the search so a dense quotient cannot run away.
pub(crate) fn for_each_class_cycle(
    graph: &Graph,
    domain: &[bool],
    max_length: usize,
    max_cycles: usize,
    max_literals: usize,
    deadline: Option<Instant>,
    emit: &mut impl FnMut(&[VertexId], &[&[VertexId]]),
) -> ClassCycleEnumeration {
    if max_length == 0 {
        return ClassCycleEnumeration::default();
    }
    let adj = quotient(graph, domain);
    let mut walk = Walk {
        target: 0,
        max_cycles,
        max_literals,
        deadline,
        visited: vec![false; adj.len()],
        path: Vec::new(),
        cycles: 0,
        literals: 0,
        visits: 0,
        reached: 0,
        truncated: false,
    };
    let mut path_members = Vec::new();
    for length in 1..=max_length {
        walk.target = length;
        walk.reached = length;
        for start in 0..adj.len() {
            if adj[start].is_empty() {
                continue;
            }
            walk.visited[start] = true;
            walk.path.push(start);
            walk.walk(&adj, &mut path_members, start, start, emit);
            walk.path.pop();
            walk.visited[start] = false;
            if walk.truncated {
                break;
            }
        }
        if walk.truncated {
            break;
        }
    }
    ClassCycleEnumeration {
        cycles: walk.cycles,
        literals: walk.literals,
        visited: walk.visits,
        ceiling: max_length,
        reached: walk.reached,
        truncated: walk.truncated,
    }
}

struct Walk {
    target: usize,
    max_cycles: usize,
    max_literals: usize,
    deadline: Option<Instant>,
    visited: Vec<bool>,
    path: Vec<VertexId>,
    cycles: usize,
    literals: usize,
    visits: usize,
    reached: usize,
    truncated: bool,
}

impl Walk {
    fn walk<'a>(
        &mut self,
        adj: &'a [Vec<(VertexId, Vec<VertexId>)>],
        path_members: &mut Vec<&'a [VertexId]>,
        start: VertexId,
        node: VertexId,
        emit: &mut impl FnMut(&[VertexId], &[&[VertexId]]),
    ) {
        for (next, members) in &adj[node] {
            self.visits += 1;
            if self.visits.is_multiple_of(4096)
                && self
                    .deadline
                    .is_some_and(|deadline| Instant::now() >= deadline)
            {
                self.truncated = true;
                return;
            }
            if *next == start {
                if self.path.len() == self.target {
                    path_members.push(members);
                    self.emit_cycle(path_members, emit);
                    path_members.pop();
                    if self.truncated {
                        return;
                    }
                }
            } else if *next > start && !self.visited[*next] && self.path.len() < self.target {
                self.visited[*next] = true;
                self.path.push(*next);
                path_members.push(members);
                self.walk(adj, path_members, start, *next, emit);
                path_members.pop();
                self.path.pop();
                self.visited[*next] = false;
                if self.truncated {
                    return;
                }
            }
        }
    }

    fn emit_cycle(
        &mut self,
        members: &[&[VertexId]],
        emit: &mut impl FnMut(&[VertexId], &[&[VertexId]]),
    ) {
        let literals: usize = members.iter().map(|set| set.len()).sum();
        if self.cycles >= self.max_cycles || self.literals + literals > self.max_literals {
            self.truncated = true;
            return;
        }
        emit(&self.path, members);
        self.cycles += 1;
        self.literals += literals;
    }
}

/// Class-level adjacency. Members are ordered by constructor id, children by
/// class id, so enumeration is deterministic for a given graph.
fn quotient(graph: &Graph, domain: &[bool]) -> Vec<Vec<(VertexId, Vec<VertexId>)>> {
    let mut adj = vec![Vec::new(); graph.graph.node_count()];
    for class in graph.graph.node_indices() {
        let class_id = class.index();
        if !domain[class_id] || graph.graph[class].is_all {
            continue;
        }
        let mut by_child: BTreeMap<VertexId, Vec<VertexId>> = BTreeMap::new();
        for ctor in graph.graph.neighbors(class) {
            let ctor_id = ctor.index();
            if !domain[ctor_id] || !graph.graph[ctor].is_all {
                continue;
            }
            for child in graph.graph.neighbors(ctor) {
                let child_id = child.index();
                if !domain[child_id] || graph.graph[child].is_all {
                    continue;
                }
                let members = by_child.entry(child_id).or_default();
                if !members.contains(&ctor_id) {
                    members.push(ctor_id);
                }
            }
        }
        adj[class_id] = by_child.into_iter().collect();
    }
    adj
}
