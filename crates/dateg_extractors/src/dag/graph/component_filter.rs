//! Top-down component filter.
//!
//! The filter works on pure components only and applies two local rules to a
//! fixpoint on the current enabled view:
//!
//! - **A**: a constructor `t` is removed when another enabled alternative `p`
//!   of the same class is no more expensive and all of `p`'s inputs are
//!   required by every way that uses `t` (`inputs(p) ⊆ must(t)`).
//! - **B** (optional, default off): a constructor `t` is removed together with
//!   its private cone when another alternative `p` is no more expensive once
//!   the private cone's saved cost is credited.
//!
//! Both rules only disable vertices. They are applied to the current view and
//! recomputed after each batch; no certificate from an earlier round is kept.

use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

use petgraph::{
    Direction::{Incoming, Outgoing},
    graph::NodeIndex,
};

use super::{Graph, VertexId, components};

const INF: usize = usize::MAX / 4;
const MAX_ROUNDS: usize = 64;

#[derive(Clone, Debug, Default)]
pub(crate) struct Report {
    pub(crate) disabled: Vec<bool>,
    pub(crate) components: usize,
    pub(crate) vertices: usize,
    pub(crate) inputs: usize,
    pub(crate) outputs: usize,
    pub(crate) rounds: usize,
    pub(crate) removed_a: usize,
    pub(crate) removed_b: usize,
    pub(crate) removed_cascade: usize,
    pub(crate) disabled_total: usize,
    pub(crate) timed_out: bool,
    pub(crate) duration: Duration,
}

/// Component-filter options. The private-cone rule is off by default: it
/// removes few vertices on the current corpus and forces many full recompute
/// rounds. It is kept for experiments with support-aware bounds.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Config {
    pub(crate) private_cones: bool,
}

pub(crate) fn filter(
    graph: &Graph,
    roots: &[VertexId],
    initial: &[bool],
    deadline: Option<Instant>,
    config: Config,
) -> Report {
    let started = Instant::now();
    let regions = components::analyze(graph);
    let count = graph.graph.node_count();
    let mut enabled = initial.to_vec();
    let mut disabled = vec![false; count];
    let mut report = Report {
        disabled: Vec::new(),
        components: regions.len(),
        vertices: regions.iter().map(|r| r.vertices.len()).sum(),
        inputs: regions.iter().map(|r| r.inputs.len()).sum(),
        outputs: regions.iter().map(|r| r.outputs.len()).sum(),
        ..Report::default()
    };

    let mut rounds = 0;
    loop {
        rounds += 1;
        let mut changed = false;
        for region in &regions {
            if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                report.timed_out = true;
                break;
            }
            let state = State::new(graph, region, &enabled, roots, config);
            let removed = state.rule_a();
            if !removed.is_empty() {
                for vertex in removed {
                    if enabled[vertex] {
                        enabled[vertex] = false;
                        disabled[vertex] = true;
                        report.removed_a += 1;
                    }
                }
                report.removed_cascade +=
                    cascade(graph, region, roots, &mut enabled, &mut disabled);
                changed = true;
                continue;
            }

            if let Some(removed) = state.rule_b() {
                for vertex in removed {
                    if enabled[vertex] {
                        enabled[vertex] = false;
                        disabled[vertex] = true;
                        report.removed_b += 1;
                    }
                }
                report.removed_cascade +=
                    cascade(graph, region, roots, &mut enabled, &mut disabled);
                changed = true;
            }
        }
        if report.timed_out || !changed || rounds >= MAX_ROUNDS {
            break;
        }
    }

    report.rounds = rounds;
    report.disabled_total = disabled.iter().filter(|&&d| d).count();
    report.disabled = disabled;
    report.duration = started.elapsed();
    report
}

fn is_value(graph: &Graph, vertex: VertexId) -> bool {
    !graph.graph[NodeIndex::new(vertex)].is_all
}

fn is_pure_constructor(graph: &Graph, vertex: VertexId) -> bool {
    let node = &graph.graph[NodeIndex::new(vertex)];
    node.is_all && node.consumes.is_empty()
}

fn inputs(graph: &Graph, constructor: VertexId) -> impl Iterator<Item = VertexId> + '_ {
    graph
        .graph
        .neighbors_directed(NodeIndex::new(constructor), Outgoing)
        .map(|node| node.index())
}

fn producers(graph: &Graph, value: VertexId) -> impl Iterator<Item = VertexId> + '_ {
    graph
        .graph
        .neighbors_directed(NodeIndex::new(value), Outgoing)
        .map(|node| node.index())
}

fn consumers(graph: &Graph, value: VertexId) -> impl Iterator<Item = VertexId> + '_ {
    graph
        .graph
        .neighbors_directed(NodeIndex::new(value), Incoming)
        .map(|node| node.index())
}

fn output_class(graph: &Graph, constructor: VertexId) -> Option<VertexId> {
    graph
        .graph
        .neighbors_directed(NodeIndex::new(constructor), Incoming)
        .find(|&node| !graph.graph[node].is_all)
        .map(|node| node.index())
}

fn cost(graph: &Graph, vertex: VertexId) -> usize {
    graph.graph[NodeIndex::new(vertex)].cost
}

fn full_mask(width: usize, len: usize) -> Vec<u64> {
    let mut mask = vec![u64::MAX; width];
    if width > 0 {
        let unused = width * 64 - len;
        if unused > 0 {
            mask[width - 1] = u64::MAX >> unused;
        }
    }
    mask
}

fn set_bit(mask: &mut [u64], index: usize) {
    mask[index / 64] |= 1u64 << (index % 64);
}

fn has_bit(mask: &[u64], index: usize) -> bool {
    mask[index / 64] & (1u64 << (index % 64)) != 0
}

struct State<'a> {
    graph: &'a Graph,
    region: &'a components::Component,
    enabled: &'a [bool],
    local: Vec<Option<usize>>,
    members: Vec<VertexId>,
    width: usize,
    must: Vec<Vec<u64>>,
    ub: Vec<usize>,
    users: Vec<u32>,
    protected: Vec<bool>,
    /// Rule B needs comparable supports; only the support-free (I empty)
    /// components are handled for now.
    allow_rule_b: bool,
}

impl<'a> State<'a> {
    fn new(
        graph: &'a Graph,
        region: &'a components::Component,
        enabled: &'a [bool],
        roots: &[VertexId],
        config: Config,
    ) -> Self {
        let count = graph.graph.node_count();
        let mut local = vec![None; count];
        for (index, &vertex) in region.vertices.iter().enumerate() {
            local[vertex] = Some(index);
        }
        let members = region.vertices.clone();
        let width = members.len().div_ceil(64);
        let mut protected = vec![false; members.len()];
        for &vertex in region
            .inputs
            .iter()
            .chain(region.outputs.iter())
            .chain(roots)
        {
            if let Some(index) = local[vertex] {
                protected[index] = true;
            }
        }
        let mut state = Self {
            graph,
            region,
            enabled,
            local,
            members,
            width,
            must: vec![Vec::new(); region.vertices.len()],
            ub: vec![INF; region.vertices.len()],
            users: vec![0; region.vertices.len()],
            protected,
            allow_rule_b: config.private_cones && region.inputs.is_empty(),
        };
        state.compute_users();
        state.compute_must();
        if state.allow_rule_b {
            state.compute_ub();
        }
        state
    }

    fn index(&self, vertex: VertexId) -> Option<usize> {
        self.local.get(vertex).copied().flatten()
    }

    fn is_leaf_value(&self, value: VertexId) -> bool {
        self.region.inputs.binary_search(&value).is_ok()
    }

    fn compute_users(&mut self) {
        for index in 0..self.members.len() {
            let vertex = self.members[index];
            if is_value(self.graph, vertex) {
                self.users[index] = consumers(self.graph, vertex)
                    .filter(|&consumer| self.enabled[consumer])
                    .count() as u32;
            }
        }
    }

    fn compute_must(&mut self) {
        let len = self.members.len();
        let universal = full_mask(self.width, len);
        let mut queued = vec![true; len];
        let mut queue: VecDeque<usize> = (0..len).collect();

        for index in 0..len {
            let vertex = self.members[index];
            let leaf = if is_value(self.graph, vertex) {
                self.is_leaf_value(vertex)
            } else {
                self.is_pure_constructor(vertex) && inputs(self.graph, vertex).next().is_none()
            };
            let mut mask = if leaf {
                vec![0u64; self.width]
            } else {
                universal.clone()
            };
            set_bit(&mut mask, index);
            self.must[index] = mask;
        }

        while let Some(index) = queue.pop_front() {
            queued[index] = false;
            let vertex = self.members[index];
            let next = if is_value(self.graph, vertex) {
                if self.is_leaf_value(vertex) {
                    let mut mask = vec![0u64; self.width];
                    set_bit(&mut mask, index);
                    mask
                } else {
                    let mut mask: Option<Vec<u64>> = None;
                    for producer in producers(self.graph, vertex) {
                        if !self.enabled[producer] || !self.is_member(producer) {
                            continue;
                        }
                        if !is_pure_constructor(self.graph, producer) {
                            continue;
                        }
                        let Some(producer_index) = self.index(producer) else {
                            continue;
                        };
                        mask = Some(match mask {
                            None => self.must[producer_index].clone(),
                            Some(mut current) => {
                                for (dst, src) in current.iter_mut().zip(&self.must[producer_index])
                                {
                                    *dst &= *src;
                                }
                                current
                            }
                        });
                    }
                    let mut mask = mask.unwrap_or_else(|| vec![0u64; self.width]);
                    set_bit(&mut mask, index);
                    mask
                }
            } else {
                let mut mask = vec![0u64; self.width];
                set_bit(&mut mask, index);
                if !self.enabled[vertex] {
                    mask
                } else {
                    let mut blocked = false;
                    for input in inputs(self.graph, vertex) {
                        if !self.enabled[input] {
                            blocked = true;
                            break;
                        }
                        let Some(input_index) = self.index(input) else {
                            continue;
                        };
                        for (dst, src) in mask.iter_mut().zip(&self.must[input_index]) {
                            *dst |= *src;
                        }
                    }
                    if blocked {
                        let mut singleton = vec![0u64; self.width];
                        set_bit(&mut singleton, index);
                        singleton
                    } else {
                        mask
                    }
                }
            };
            if next == self.must[index] {
                continue;
            }
            self.must[index] = next;

            if is_value(self.graph, vertex) {
                for consumer in consumers(self.graph, vertex) {
                    if self.enabled[consumer] {
                        if let Some(consumer_index) = self.index(consumer) {
                            self.enqueue(consumer_index, &mut queued, &mut queue);
                        }
                    }
                }
            } else {
                for output in output_classes(self.graph, vertex) {
                    if let Some(output_index) = self.index(output) {
                        self.enqueue(output_index, &mut queued, &mut queue);
                    }
                }
            }
        }
    }

    fn enqueue(&self, index: usize, queued: &mut [bool], queue: &mut VecDeque<usize>) {
        if !queued[index] {
            queued[index] = true;
            queue.push_back(index);
        }
    }

    fn is_member(&self, vertex: VertexId) -> bool {
        self.local.get(vertex).is_some_and(|local| local.is_some())
    }

    fn compute_ub(&mut self) {
        let len = self.members.len();
        let mut queued = vec![false; len];
        let mut queue: VecDeque<usize> = VecDeque::new();

        for index in 0..len {
            let vertex = self.members[index];
            if !self.enabled[vertex] {
                continue;
            }
            if is_value(self.graph, vertex) {
                if self.is_leaf_value(vertex) {
                    self.ub[index] = 0;
                    self.enqueue(index, &mut queued, &mut queue);
                }
            } else if self.is_pure_constructor(vertex)
                && inputs(self.graph, vertex).next().is_none()
            {
                self.ub[index] = cost(self.graph, vertex).min(INF);
                self.enqueue(index, &mut queued, &mut queue);
            }
        }

        while let Some(index) = queue.pop_front() {
            queued[index] = false;
            let vertex = self.members[index];
            if is_value(self.graph, vertex) {
                for consumer in consumers(self.graph, vertex).collect::<Vec<_>>() {
                    if let Some(consumer_index) = self.index(consumer) {
                        self.relax_constructor(consumer_index, &mut queued, &mut queue);
                    }
                }
            } else {
                for output in output_classes(self.graph, vertex) {
                    if let Some(output_index) = self.index(output) {
                        self.relax_value(output_index, &mut queued, &mut queue);
                    }
                }
            }
        }
    }

    fn is_pure_constructor(&self, vertex: VertexId) -> bool {
        is_pure_constructor(self.graph, vertex)
    }

    /// Tree upper bounds computed with `blocked` vertices treated as
    /// unavailable. Used to build a replacement way that neither uses the
    /// removed private cone nor the target's output class.
    fn ub_avoiding(&self, blocked: &[bool]) -> Vec<usize> {
        let len = self.members.len();
        let mut ub = vec![INF; len];
        let mut queued = vec![false; len];
        let mut queue: VecDeque<usize> = VecDeque::new();

        for index in 0..len {
            if blocked[index] {
                continue;
            }
            let vertex = self.members[index];
            if !self.enabled[vertex] {
                continue;
            }
            if is_value(self.graph, vertex) {
                if self.is_leaf_value(vertex) {
                    ub[index] = 0;
                    self.enqueue(index, &mut queued, &mut queue);
                }
            } else if is_pure_constructor(self.graph, vertex)
                && inputs(self.graph, vertex).next().is_none()
            {
                ub[index] = cost(self.graph, vertex).min(INF);
                self.enqueue(index, &mut queued, &mut queue);
            }
        }

        while let Some(index) = queue.pop_front() {
            queued[index] = false;
            let vertex = self.members[index];
            if is_value(self.graph, vertex) {
                for consumer in consumers(self.graph, vertex).collect::<Vec<_>>() {
                    if !self.enabled[consumer] || !is_pure_constructor(self.graph, consumer) {
                        continue;
                    }
                    let Some(consumer_index) = self.index(consumer) else {
                        continue;
                    };
                    if blocked[consumer_index] {
                        continue;
                    }
                    let mut total = cost(self.graph, consumer);
                    let mut valid = true;
                    for input in inputs(self.graph, consumer) {
                        if !self.enabled[input] {
                            valid = false;
                            break;
                        }
                        let Some(input_index) = self.index(input) else {
                            valid = false;
                            break;
                        };
                        if blocked[input_index] || ub[input_index] >= INF {
                            valid = false;
                            break;
                        }
                        total = total.saturating_add(ub[input_index]);
                        if total >= INF {
                            valid = false;
                            break;
                        }
                    }
                    if valid {
                        let total = total.min(INF);
                        if total < ub[consumer_index] {
                            ub[consumer_index] = total;
                            self.enqueue(consumer_index, &mut queued, &mut queue);
                        }
                    }
                }
            } else {
                for output in output_classes(self.graph, vertex) {
                    let Some(output_index) = self.index(output) else {
                        continue;
                    };
                    if blocked[output_index] || !self.enabled[output] {
                        continue;
                    }
                    let mut best = INF;
                    for producer in producers(self.graph, output) {
                        if !self.enabled[producer] || !is_pure_constructor(self.graph, producer) {
                            continue;
                        }
                        let Some(producer_index) = self.index(producer) else {
                            continue;
                        };
                        if blocked[producer_index] {
                            continue;
                        }
                        best = best.min(ub[producer_index]);
                    }
                    if best < ub[output_index] {
                        ub[output_index] = best;
                        self.enqueue(output_index, &mut queued, &mut queue);
                    }
                }
            }
        }
        ub
    }

    fn relax_constructor(
        &mut self,
        index: usize,
        queued: &mut [bool],
        queue: &mut VecDeque<usize>,
    ) {
        let vertex = self.members[index];
        if !self.enabled[vertex] || !is_pure_constructor(self.graph, vertex) {
            return;
        }
        let mut total = cost(self.graph, vertex);
        for input in inputs(self.graph, vertex) {
            if !self.enabled[input] {
                return;
            }
            let Some(input_index) = self.index(input) else {
                return;
            };
            let value = self.ub[input_index];
            if value >= INF {
                return;
            }
            total = total.saturating_add(value);
            if total >= INF {
                return;
            }
        }
        let total = total.min(INF);
        if total < self.ub[index] {
            self.ub[index] = total;
            self.enqueue(index, queued, queue);
        }
    }

    fn relax_value(&mut self, index: usize, queued: &mut [bool], queue: &mut VecDeque<usize>) {
        let vertex = self.members[index];
        if !self.enabled[vertex] || !is_value(self.graph, vertex) {
            return;
        }
        if self.is_leaf_value(vertex) {
            return;
        }
        let mut best = INF;
        for producer in producers(self.graph, vertex) {
            if !self.enabled[producer] || !is_pure_constructor(self.graph, producer) {
                continue;
            }
            let Some(producer_index) = self.index(producer) else {
                continue;
            };
            best = best.min(self.ub[producer_index]);
        }
        if best < self.ub[index] {
            self.ub[index] = best;
            self.enqueue(index, queued, queue);
        }
    }

    fn inputs_subset_must(&self, candidate: VertexId, target: VertexId) -> bool {
        let Some(target_index) = self.index(target) else {
            return false;
        };
        let must = &self.must[target_index];
        inputs(self.graph, candidate).all(|input| {
            self.index(input)
                .is_some_and(|input_index| has_bit(must, input_index))
        })
    }

    fn must_size(&self, vertex: VertexId) -> u32 {
        self.index(vertex)
            .map(|index| self.must[index].iter().map(|word| word.count_ones()).sum())
            .unwrap_or(u32::MAX)
    }

    fn rule_a(&self) -> Vec<VertexId> {
        let mut removed = Vec::new();
        for index in 0..self.members.len() {
            let class = self.members[index];
            if !self.enabled[class] || !is_value(self.graph, class) {
                continue;
            }
            let mut candidates: Vec<VertexId> = producers(self.graph, class)
                .filter(|&producer| {
                    self.enabled[producer]
                        && self.is_member(producer)
                        && is_pure_constructor(self.graph, producer)
                })
                .collect();
            if candidates.len() < 2 {
                continue;
            }
            candidates.sort_unstable_by_key(|&candidate| {
                (
                    inputs(self.graph, candidate).count(),
                    self.must_size(candidate),
                    cost(self.graph, candidate),
                    candidate,
                )
            });
            let mut kept: Vec<VertexId> = Vec::new();
            for candidate in candidates {
                let dominated = kept.iter().any(|&keeper| {
                    cost(self.graph, keeper) <= cost(self.graph, candidate)
                        && self.inputs_subset_must(keeper, candidate)
                });
                if dominated {
                    removed.push(candidate);
                } else {
                    kept.push(candidate);
                }
            }
        }
        removed
    }

    fn rule_b(&self) -> Option<Vec<VertexId>> {
        if !self.allow_rule_b {
            return None;
        }
        for index in 0..self.members.len() {
            let target = self.members[index];
            if !self.enabled[target] || !is_pure_constructor(self.graph, target) {
                continue;
            }
            let Some(class) = output_class(self.graph, target) else {
                continue;
            };
            let Some(class_index) = self.index(class) else {
                continue;
            };
            let (private_classes, private_constructors) = self.private_cone(target);
            if private_classes[class_index] {
                continue;
            }
            let Some(private_cost) =
                self.private_cost(target, &private_classes, &private_constructors)
            else {
                continue;
            };

            let mut blocked = private_classes.clone();
            for (constructor_index, &private) in private_constructors.iter().enumerate() {
                if private {
                    blocked[constructor_index] = true;
                }
            }
            blocked[class_index] = true;
            let avoiding = self.ub_avoiding(&blocked);

            let mut free = vec![false; self.members.len()];
            let must = &self.must[index];
            for (word_index, &word) in must.iter().enumerate() {
                let mut word = word;
                while word != 0 {
                    let bit = word.trailing_zeros() as usize;
                    free[word_index * 64 + bit] = true;
                    word &= word - 1;
                }
            }
            for input in inputs(self.graph, target) {
                if let Some(input_index) = self.index(input) {
                    if !private_classes[input_index] {
                        free[input_index] = true;
                    }
                }
            }
            for constructor_index in 0..self.members.len() {
                if !private_constructors[constructor_index] {
                    continue;
                }
                for input in inputs(self.graph, self.members[constructor_index]) {
                    if let Some(input_index) = self.index(input) {
                        if !private_classes[input_index] {
                            free[input_index] = true;
                        }
                    }
                }
            }

            let mut alternatives: Vec<VertexId> = producers(self.graph, class)
                .filter(|&producer| {
                    producer != target
                        && self.enabled[producer]
                        && self.is_member(producer)
                        && is_pure_constructor(self.graph, producer)
                })
                .collect();
            alternatives.sort_unstable();

            for alternative in alternatives {
                let alternative_index = self.index(alternative).expect("member alternative");
                if private_constructors[alternative_index] {
                    continue;
                }
                let mut total = cost(self.graph, alternative);
                let mut valid = true;
                for input in inputs(self.graph, alternative) {
                    let Some(input_index) = self.index(input) else {
                        valid = false;
                        break;
                    };
                    if private_classes[input_index] {
                        valid = false;
                        break;
                    }
                    if input == class || has_bit(&self.must[input_index], class_index) {
                        valid = false;
                        break;
                    }
                    if free[input_index] {
                        continue;
                    }
                    let value = avoiding[input_index];
                    if value >= INF {
                        valid = false;
                        break;
                    }
                    total = total.saturating_add(value);
                    if total >= INF {
                        valid = false;
                        break;
                    }
                }
                if !valid {
                    continue;
                }
                if total <= cost(self.graph, target).saturating_add(private_cost) {
                    let mut removed = Vec::new();
                    removed.push(target);
                    for (member_index, &member) in self.members.iter().enumerate() {
                        if private_constructors[member_index] || private_classes[member_index] {
                            removed.push(member);
                        }
                    }
                    return Some(removed);
                }
            }
        }
        None
    }

    fn private_cone(&self, target: VertexId) -> (Vec<bool>, Vec<bool>) {
        let len = self.members.len();
        let mut private_classes = vec![false; len];
        let mut private_constructors = vec![false; len];
        let mut in_cone_users = vec![0u32; len];
        let mut queue: VecDeque<usize> = VecDeque::new();

        let add_class = |index: usize, classes: &mut Vec<bool>, queue: &mut VecDeque<usize>| {
            if !classes[index] && !self.protected[index] {
                classes[index] = true;
                queue.push_back(index);
            }
        };

        for input in inputs(self.graph, target) {
            let Some(input_index) = self.index(input) else {
                continue;
            };
            if self.protected[input_index] {
                continue;
            }
            in_cone_users[input_index] += 1;
            if in_cone_users[input_index] == self.users[input_index] {
                add_class(input_index, &mut private_classes, &mut queue);
            }
        }

        while let Some(class_index) = queue.pop_front() {
            let class = self.members[class_index];
            for producer in producers(self.graph, class) {
                if !self.enabled[producer] || !is_pure_constructor(self.graph, producer) {
                    continue;
                }
                let Some(producer_index) = self.index(producer) else {
                    continue;
                };
                if private_constructors[producer_index] {
                    continue;
                }
                private_constructors[producer_index] = true;
                for input in inputs(self.graph, producer) {
                    let Some(input_index) = self.index(input) else {
                        continue;
                    };
                    if private_classes[input_index] || self.protected[input_index] {
                        continue;
                    }
                    in_cone_users[input_index] += 1;
                    if in_cone_users[input_index] == self.users[input_index] {
                        add_class(input_index, &mut private_classes, &mut queue);
                    }
                }
            }
        }
        (private_classes, private_constructors)
    }

    fn private_cost(
        &self,
        target: VertexId,
        private_classes: &[bool],
        private_constructors: &[bool],
    ) -> Option<usize> {
        let len = self.members.len();
        let mut down = vec![0usize; len];
        for index in 0..len {
            if private_classes[index] || private_constructors[index] {
                down[index] = INF;
            }
        }

        let mut changed = true;
        let mut iterations = 0;
        while changed && iterations <= len + 1 {
            changed = false;
            iterations += 1;
            for index in 0..len {
                let vertex = self.members[index];
                let mut value = down[index];
                if private_constructors[index] {
                    if !self.enabled[vertex] || !is_pure_constructor(self.graph, vertex) {
                        continue;
                    }
                    let mut current = cost(self.graph, vertex);
                    let mut blocked = false;
                    for input in inputs(self.graph, vertex) {
                        let Some(input_index) = self.index(input) else {
                            continue;
                        };
                        if private_classes[input_index] {
                            if down[input_index] >= INF {
                                blocked = true;
                                break;
                            }
                            current = current.max(down[input_index]);
                        }
                    }
                    value = if blocked { INF } else { current.min(INF) };
                } else if private_classes[index] {
                    let mut best = INF;
                    for producer in producers(self.graph, vertex) {
                        if !self.enabled[producer] || !is_pure_constructor(self.graph, producer) {
                            continue;
                        }
                        let Some(producer_index) = self.index(producer) else {
                            continue;
                        };
                        if private_constructors[producer_index] {
                            best = best.min(down[producer_index]);
                        }
                    }
                    value = best;
                }
                if value < down[index] {
                    down[index] = value;
                    changed = true;
                }
            }
        }

        let mut saved = 0;
        for input in inputs(self.graph, target) {
            let Some(input_index) = self.index(input) else {
                continue;
            };
            if private_classes[input_index] {
                if down[input_index] >= INF {
                    return None;
                }
                saved = saved.max(down[input_index]);
            }
        }
        Some(saved)
    }
}

fn output_classes<'a>(
    graph: &'a Graph,
    constructor: VertexId,
) -> impl Iterator<Item = VertexId> + 'a {
    graph
        .graph
        .neighbors_directed(NodeIndex::new(constructor), Incoming)
        .filter(|&node| !graph.graph[node].is_all)
        .map(|node| node.index())
}

fn cascade(
    graph: &Graph,
    region: &components::Component,
    roots: &[VertexId],
    enabled: &mut [bool],
    disabled: &mut [bool],
) -> usize {
    let count = graph.graph.node_count();
    let mut protected = vec![false; count];
    for &vertex in region
        .inputs
        .iter()
        .chain(region.outputs.iter())
        .chain(roots)
    {
        protected[vertex] = true;
    }
    let mut removed = 0;
    loop {
        let mut changed = false;
        for &vertex in &region.vertices {
            if !enabled[vertex] {
                continue;
            }
            if is_value(graph, vertex) {
                if protected[vertex] {
                    continue;
                }
                if consumers(graph, vertex).any(|consumer| enabled[consumer]) {
                    continue;
                }
                enabled[vertex] = false;
                disabled[vertex] = true;
                removed += 1;
                changed = true;
            } else if is_pure_constructor(graph, vertex) {
                let inputs_enabled = inputs(graph, vertex).all(|input| enabled[input]);
                let output_enabled = output_class(graph, vertex).is_none_or(|class| enabled[class]);
                if inputs_enabled && output_enabled {
                    continue;
                }
                enabled[vertex] = false;
                disabled[vertex] = true;
                removed += 1;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(graph: &mut Graph) -> VertexId {
        graph.add_vertex(false, 0, || None)
    }

    fn constructor(graph: &mut Graph, inputs: &[VertexId]) -> VertexId {
        constructor_cost(graph, 1, inputs)
    }

    fn constructor_cost(graph: &mut Graph, cost: usize, inputs: &[VertexId]) -> VertexId {
        let constructor = graph.add_vertex(true, cost, || None);
        graph.add_edges(constructor, inputs.iter().copied());
        constructor
    }

    fn input(graph: &mut Graph, memory: VertexId) -> VertexId {
        let value = value(graph);
        let load = graph.add_vertex(true, 1, || None);
        graph.set_consumes(load, [memory]);
        graph.add_edges(value, [load]);
        value
    }

    fn constant(graph: &mut Graph) -> VertexId {
        let value = value(graph);
        let constant = constructor_cost(graph, 0, &[]);
        graph.add_edges(value, [constant]);
        value
    }

    fn disabled(report: &Report, vertex: VertexId) -> bool {
        report.disabled[vertex]
    }

    #[test]
    fn cheaper_direct_alternative_removes_the_detour() {
        let mut graph = Graph::default();
        let memory = value(&mut graph);
        let a = input(&mut graph, memory);
        let b = input(&mut graph, memory);

        let negated = value(&mut graph);
        let neg = constructor(&mut graph, &[b]);
        graph.add_edges(negated, [neg]);

        let root = value(&mut graph);
        let detour = constructor(&mut graph, &[a, negated]);
        let direct = constructor(&mut graph, &[a, b]);
        graph.add_edges(root, [detour]);
        graph.add_edges(root, [direct]);

        let report = filter(
            &graph,
            &[root],
            &vec![true; graph.graph.node_count()],
            None,
            Config::default(),
        );
        assert!(disabled(&report, detour), "detour should be removed");
        assert!(!disabled(&report, direct), "direct alternative must stay");
        assert!(
            disabled(&report, negated),
            "private input class should cascade"
        );
        assert!(disabled(&report, neg), "private producer should cascade");
        assert!(!disabled(&report, root));
        assert!(!disabled(&report, a));
        assert!(!disabled(&report, b));
    }

    #[test]
    fn private_cone_can_be_removed_for_associativity() {
        let mut graph = Graph::default();
        let a = constant(&mut graph);
        let b = constant(&mut graph);
        let c = constant(&mut graph);

        let bc = value(&mut graph);
        let make_bc = constructor(&mut graph, &[b, c]);
        graph.add_edges(bc, [make_bc]);

        let ab = value(&mut graph);
        let make_ab = constructor(&mut graph, &[a, b]);
        graph.add_edges(ab, [make_ab]);

        let root = value(&mut graph);
        let left = constructor(&mut graph, &[a, bc]);
        let right = constructor(&mut graph, &[ab, c]);
        graph.add_edges(root, [left]);
        graph.add_edges(root, [right]);

        let report = filter(
            &graph,
            &[root],
            &vec![true; graph.graph.node_count()],
            None,
            Config {
                private_cones: true,
            },
        );
        assert!(
            disabled(&report, left),
            "left private cone should be removed"
        );
        assert!(
            disabled(&report, bc),
            "private intermediate should be removed"
        );
        assert!(
            disabled(&report, make_bc),
            "private producer should be removed"
        );
        assert!(
            !disabled(&report, right),
            "replacement alternative must stay"
        );
        assert!(!disabled(&report, ab));
        assert!(!disabled(&report, make_ab));
        assert!(!disabled(&report, root));
    }

    #[test]
    fn shared_input_is_not_credited() {
        let mut graph = Graph::default();
        let a = constant(&mut graph);
        let b = constant(&mut graph);
        let c = constant(&mut graph);
        let d = constant(&mut graph);

        let bc = value(&mut graph);
        let make_bc = constructor(&mut graph, &[b, c]);
        graph.add_edges(bc, [make_bc]);

        let ab = value(&mut graph);
        let make_ab = constructor_cost(&mut graph, 5, &[a, b]);
        graph.add_edges(ab, [make_ab]);

        let root = value(&mut graph);
        let left = constructor(&mut graph, &[a, bc]);
        let right = constructor(&mut graph, &[ab, c]);
        graph.add_edges(root, [left]);
        graph.add_edges(root, [right]);

        let other = value(&mut graph);
        let other_use = constructor(&mut graph, &[bc, d]);
        graph.add_edges(other, [other_use]);

        let report = filter(
            &graph,
            &[root, other],
            &vec![true; graph.graph.node_count()],
            None,
            Config {
                private_cones: true,
            },
        );
        assert!(
            !disabled(&report, left),
            "shared boundary must not be credited to the left cone"
        );
    }
}
