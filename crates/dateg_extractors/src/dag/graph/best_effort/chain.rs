//! Greedy chain extraction with bounded rollbacks.
//!
//! The solver exploits the shape of a stateful theory: one class is produced
//! without consuming anything (the base), every constructor has at most one
//! consumable input, and a valid extraction selects a chain of consumers from
//! the base to the root. Reads are pure constructors: they take a consumed
//! class as input but do not consume it, so they attach to the chain and are
//! only constrained by acyclicity.
//!
//! The search walks the chain forward from the base. A candidate consumer is
//! usable only if all of its inputs are already derivable, i.e. its pure
//! arguments can be produced from the chain prefix built so far. That makes
//! reads sound by construction: a read of a later (or off-chain) class is not
//! derivable yet, so the candidate is rejected. Adding the produced class then
//! extends the pure closure through readers.
//!
//! Candidates are tried cheapest store first, then by remaining distance to the
//! root, then by id. On failure the last choices are undone (bounded number of
//! rollbacks) and the next candidate is tried. A shape check failure or an
//! exhausted budget returns `Unsolved`; the exact solvers are the next stage.

use std::{collections::VecDeque, time::Instant};

use petgraph::{Direction::Outgoing, graph::NodeIndex};

use super::super::{Graph, VertexId, domination};
use crate::dag::{ExtractError, StatsSink, check_deadline};

/// Candidate rejections before giving up.
const MAX_ROLLBACKS: u64 = 1_048_576;
/// Deadline check interval.
const CHECK_EVERY: u64 = 256;

pub(super) fn solve(
    graph: &Graph,
    deadline: Option<Instant>,
    stats: &mut StatsSink<'_>,
) -> Result<Vec<bool>, ExtractError> {
    check_deadline(deadline)?;
    let tables = Tables::new(graph);
    let Some(shape) = Shape::detect(graph, &tables) else {
        stats.text("chain.shape", "unsupported");
        return Err(ExtractError::Unsolved);
    };
    stats.text("chain.shape", "ok");

    let started = Instant::now();
    let mut attempt = Attempt::new(graph, shape, tables);
    let outcome = attempt.run(deadline, stats);
    stats.duration("chain.search", started.elapsed());
    stats.count("chain.attempts", attempt.attempts);
    stats.count("chain.rollbacks", attempt.rollbacks);
    stats.count(
        "chain.available",
        attempt.available.iter().filter(|&&value| value).count() as u64,
    );
    let assignment = outcome?;
    if assignment.is_none() {
        stats.text("chain.result", "unsolved");
    }
    assignment.ok_or(ExtractError::Unsolved)
}

/// Immutable adjacency derived from the used constructors.
struct Tables {
    /// Constructor -> produced class.
    output: Vec<Option<VertexId>>,
    /// Constructor -> consumed classes (enabled only).
    consumes: Vec<Vec<VertexId>>,
    /// Class -> constructors producing it.
    producers: Vec<Vec<VertexId>>,
    /// Class -> constructors taking it as input.
    users: Vec<Vec<VertexId>>,
    /// Constructor -> number of inputs that are not available initially;
    /// `u32::MAX` when the constructor has an unused input and can never run.
    initial_missing: Vec<u32>,
    /// Input-free non-consuming constructors: the initial closure seeds.
    input_free: Vec<VertexId>,
    /// False when some constructor has several consumable inputs or consumes
    /// several classes (the chain shape does not apply).
    shape_ok: bool,
}

impl Tables {
    fn new(graph: &Graph) -> Self {
        let count = graph.graph.node_count();
        let mut output = vec![None; count];
        let mut consumes = vec![Vec::new(); count];
        let mut producers = vec![Vec::new(); count];
        let mut users = vec![Vec::new(); count];
        let mut initial_missing = vec![0u32; count];
        let mut consumable = vec![false; count];
        let mut input_free = Vec::new();

        for node in graph.graph.node_indices() {
            if !graph.used[node.index()] || !graph.graph[node].is_all {
                continue;
            }
            let id = node.index();
            output[id] = domination::output_class(graph, id);
            consumes[id] = graph.graph[node]
                .consumes
                .iter()
                .copied()
                .filter(|&value| graph.used[value])
                .collect();
            for &value in &consumes[id] {
                consumable[value] = true;
            }
            if let Some(class) = output[id] {
                producers[class].push(id);
            }

            let mut unavailable = 0u32;
            let mut unusable = false;
            for input in graph.graph.neighbors_directed(node, Outgoing) {
                if graph.used[input.index()] {
                    unavailable += 1;
                    users[input.index()].push(id);
                } else {
                    unusable = true;
                }
            }
            initial_missing[id] = if unusable { u32::MAX } else { unavailable };
            if initial_missing[id] == 0 && consumes[id].is_empty() {
                input_free.push(id);
            }
        }

        let mut shape_ok = true;
        for node in graph.graph.node_indices() {
            if !graph.used[node.index()] || !graph.graph[node].is_all {
                continue;
            }
            let id = node.index();
            if consumes[id].len() > 1 {
                shape_ok = false;
                break;
            }
            let consumable_inputs = graph
                .graph
                .neighbors_directed(node, Outgoing)
                .filter(|&next| graph.used[next.index()])
                .filter(|&next| consumable[next.index()])
                .count();
            if consumable_inputs > 1 {
                shape_ok = false;
                break;
            }
        }

        Self {
            output,
            consumes,
            producers,
            users,
            initial_missing,
            input_free,
            shape_ok,
        }
    }
}

/// Static shape of the consumable part of the graph.
struct Shape {
    base: VertexId,
    root: VertexId,
    /// Consumers of each class, ordered by cost, distance, then id.
    consumers: Vec<Vec<VertexId>>,
    /// Remaining stores from a class to the root; `u32::MAX` if unreachable.
    distance: Vec<u32>,
}

impl Shape {
    fn detect(graph: &Graph, tables: &Tables) -> Option<Self> {
        let count = graph.graph.node_count();
        if graph.roots.len() != 1 {
            return None;
        }
        let root = graph.roots[0];
        if graph.graph[NodeIndex::new(root)].is_all {
            return None;
        }

        if !tables.shape_ok {
            return None;
        }

        // Versions that can reach the root.
        let mut cone = vec![false; count];
        let mut stack = vec![root];
        while let Some(version) = stack.pop() {
            if cone[version] {
                continue;
            }
            cone[version] = true;
            for &producer in &tables.producers[version] {
                for &consumed in &tables.consumes[producer] {
                    if !cone[consumed] {
                        stack.push(consumed);
                    }
                }
            }
        }

        // Exactly one base: a cone class produced without consuming anything.
        let mut base = None;
        for version in (0..count).filter(|&version| cone[version]) {
            if !tables.producers[version]
                .iter()
                .any(|&producer| tables.consumes[producer].is_empty())
            {
                continue;
            }
            if base.is_some() {
                return None;
            }
            base = Some(version);
        }
        let base = base?;

        // Stores left to the root, via reverse breadth-first search.
        let mut distance = vec![u32::MAX; count];
        distance[root] = 0;
        let mut queue = VecDeque::from([root]);
        while let Some(version) = queue.pop_front() {
            for &producer in &tables.producers[version] {
                for &consumed in &tables.consumes[producer] {
                    if distance[consumed] == u32::MAX {
                        distance[consumed] = distance[version] + 1;
                        queue.push_back(consumed);
                    }
                }
            }
        }

        let mut consumers: Vec<Vec<VertexId>> = vec![Vec::new(); count];
        for node in graph.graph.node_indices() {
            if !graph.used[node.index()] || !graph.graph[node].is_all {
                continue;
            }
            let producer = node.index();
            for &consumed in &tables.consumes[producer] {
                if cone[consumed] {
                    consumers[consumed].push(producer);
                }
            }
        }
        for list in consumers.iter_mut() {
            list.sort_unstable_by_key(|&producer| {
                let produced = tables.output[producer].unwrap_or(usize::MAX);
                (
                    graph.graph[NodeIndex::new(producer)].cost,
                    distance[produced],
                    produced,
                    producer,
                )
            });
        }

        Some(Self {
            base,
            root,
            consumers,
            distance,
        })
    }
}

struct Frame {
    current: VertexId,
    next: usize,
    /// Trail length before the candidate that produced this frame.
    mark: usize,
}

/// Mutable search state: availability, chosen producers, and their undo trail.
struct Attempt<'a> {
    graph: &'a Graph,
    shape: Shape,
    tables: Tables,
    available: Vec<bool>,
    chosen: Vec<Option<VertexId>>,
    missing: Vec<u32>,
    trail: Vec<Action>,
    attempts: u64,
    rollbacks: u64,
}

#[derive(Clone, Copy)]
enum Action {
    Available(VertexId),
    Chosen(VertexId),
    Missing(VertexId, u32),
}

impl<'a> Attempt<'a> {
    fn new(graph: &'a Graph, shape: Shape, tables: Tables) -> Self {
        let count = graph.graph.node_count();
        let mut attempt = Self {
            graph,
            shape,
            missing: tables.initial_missing.clone(),
            tables,
            available: vec![false; count],
            chosen: vec![None; count],
            trail: Vec::new(),
            attempts: 0,
            rollbacks: 0,
        };
        attempt.seed();
        attempt
    }

    /// Make every class produced by an input-free non-consuming constructor
    /// available, then close the pure part.
    fn seed(&mut self) {
        for index in 0..self.tables.input_free.len() {
            let constructor = self.tables.input_free[index];
            if let Some(class) = self.tables.output[constructor] {
                self.make_available(class, Some(constructor));
            }
        }
    }

    fn make_available(&mut self, class: VertexId, producer: Option<VertexId>) {
        if self.available[class] {
            return;
        }
        let mut queue = vec![class];
        self.available[class] = true;
        self.trail.push(Action::Available(class));
        if let Some(producer) = producer
            && self.chosen[class].is_none()
        {
            self.chosen[class] = Some(producer);
            self.trail.push(Action::Chosen(class));
        }
        while let Some(class) = queue.pop() {
            for index in 0..self.tables.users[class].len() {
                let user = self.tables.users[class][index];
                let missing = self.missing[user];
                if missing == 0 || missing == u32::MAX {
                    continue;
                }
                self.missing[user] = missing - 1;
                self.trail.push(Action::Missing(user, missing));
                if self.missing[user] != 0 || !self.tables.consumes[user].is_empty() {
                    continue;
                }
                let Some(produced) = self.tables.output[user] else {
                    continue;
                };
                if self.available[produced] {
                    continue;
                }
                self.available[produced] = true;
                self.trail.push(Action::Available(produced));
                if self.chosen[produced].is_none() {
                    self.chosen[produced] = Some(user);
                    self.trail.push(Action::Chosen(produced));
                }
                queue.push(produced);
            }
        }
    }

    fn rollback(&mut self, mark: usize) {
        while self.trail.len() > mark {
            match self.trail.pop().expect("trail") {
                Action::Available(class) => self.available[class] = false,
                Action::Chosen(class) => self.chosen[class] = None,
                Action::Missing(user, previous) => self.missing[user] = previous,
            }
        }
    }

    fn run(
        &mut self,
        deadline: Option<Instant>,
        stats: &mut StatsSink<'_>,
    ) -> Result<Option<Vec<bool>>, ExtractError> {
        let base = self.shape.base;
        let root = self.shape.root;
        if !self.available[base] {
            return Ok(None);
        }

        let mut stack = vec![Frame {
            current: base,
            next: 0,
            mark: self.trail.len(),
        }];
        let mut steps = 0u64;
        loop {
            steps += 1;
            if steps.is_multiple_of(CHECK_EVERY) {
                check_deadline(deadline)?;
            }

            let current = stack.last().expect("frame").current;
            if current == root {
                stats.count("chain.length", (stack.len() - 1) as u64);
                return Ok(self.materialize(root));
            }

            let next = stack.last().expect("frame").next;
            if next >= self.shape.consumers[current].len() {
                let frame = stack.pop().expect("frame");
                self.rollback(frame.mark);
                if stack.is_empty() {
                    return Ok(None);
                }
                continue;
            }
            stack.last_mut().expect("frame").next = next + 1;

            let producer = self.shape.consumers[current][next];
            self.attempts += 1;
            let usable = self.missing[producer] == 0
                && self.tables.output[producer].is_some_and(|produced| {
                    !self.available[produced] && self.shape.distance[produced] != u32::MAX
                });
            if !usable {
                self.rollbacks += 1;
                if self.rollbacks > MAX_ROLLBACKS {
                    return Ok(None);
                }
                continue;
            }

            let mark = self.trail.len();
            let produced = self.tables.output[producer].expect("checked");
            self.make_available(produced, Some(producer));
            stack.push(Frame {
                current: produced,
                next: 0,
                mark,
            });
        }
    }

    /// Collect the root's cone through the chosen producers.
    fn materialize(&self, root: VertexId) -> Option<Vec<bool>> {
        let mut selected = vec![false; self.graph.graph.node_count()];
        let mut stack = vec![root];
        while let Some(class) = stack.pop() {
            if selected[class] {
                continue;
            }
            selected[class] = true;
            let producer = self.chosen[class]?;
            if selected[producer] {
                continue;
            }
            selected[producer] = true;
            stack.extend(
                self.graph
                    .graph
                    .neighbors_directed(NodeIndex::new(producer), Outgoing)
                    .map(|next| next.index()),
            );
        }
        Some(selected)
    }
}
