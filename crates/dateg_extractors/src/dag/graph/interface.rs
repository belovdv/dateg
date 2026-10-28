//! Configuration, result, and graph-view data.

use std::time::Duration;

use petgraph::graph::DiGraph;

pub(crate) type VertexId = usize;

#[derive(Debug)]
pub(crate) struct Vertex {
    pub(crate) is_all: bool,
    pub(crate) cost: usize,
    pub(crate) label: Option<String>,
    /// Input classes consumed by this constructor, if any.
    pub(crate) consumes: Vec<VertexId>,
}

/// Extraction graph: static structure only.
///
/// Node ids and adjacency order follow insertion order; `add_vertex` and
/// `add_edges` preserve it. Extraction algorithms may rely on this order for
/// deterministic tie-breaking, but must not iterate hash maps or treat
/// wall-clock cutoffs as deterministic. Root order is caller-provided.
#[derive(Default)]
pub(crate) struct Graph {
    pub(crate) graph: DiGraph<Vertex, ()>,
    pub(crate) conflict_groups: Vec<Vec<VertexId>>,
}

/// Per-solve constraints. Roots are supplied separately.
pub(crate) struct View {
    pub(crate) enabled: Vec<bool>,
    pub(crate) extra_groups: Vec<Vec<VertexId>>,
}

/// How cyclic models are refined.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CycleBlocking {
    /// Block every vertex of each cyclic SCC.
    Scc,
    /// Block one shortest cycle per cyclic SCC (default).
    #[default]
    Shortest,
}

/// Which best-effort algorithm to run.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BestEffortAlgorithm {
    /// Greedy chain walk with bounded rollbacks.
    #[default]
    Chain,
}

/// Configuration for the best-effort solver family.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BestEffortConfig {
    pub algorithm: BestEffortAlgorithm,
}

impl BestEffortConfig {
    pub fn new(algorithm: BestEffortAlgorithm) -> Self {
        Self { algorithm }
    }
}

/// Pre-asserted class-cycle clauses (a subset of the exact acyclicity encoding).
///
/// A class cycle is a cycle of e-classes in the quotient graph. One clause per
/// class cycle rules out every constructor combination that realizes it, so a
/// length bound covers many node cycles at once without enumerating them.
/// Lengths are visited shortest-first until a budget runs out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreemptiveCyclesConfig {
    /// Longest class cycle to encode. `None` derives the length from the
    /// budgets, up to an internal ceiling of 6. `Some(0)` disables the pass.
    pub max_length: Option<usize>,
    /// Absolute clause cap. `None` uses the cycle rank of the reachable graph.
    pub max_cycles: Option<usize>,
    /// Absolute literal cap. `None` uses `10 *` the reachable vertex count.
    pub max_literals: Option<usize>,
}

impl Default for PreemptiveCyclesConfig {
    fn default() -> Self {
        Self {
            max_length: None,
            max_cycles: None,
            max_literals: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MaxSatIncSccConfig {
    /// Class-cycle clauses asserted before the first solve.
    pub preemptive_cycles: Option<PreemptiveCyclesConfig>,
    /// How cyclic models are refined.
    pub cycle_blocking: CycleBlocking,
}

impl Default for MaxSatIncSccConfig {
    fn default() -> Self {
        Self {
            preemptive_cycles: Some(PreemptiveCyclesConfig::default()),
            cycle_blocking: CycleBlocking::default(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SatDescendingConfig {
    /// Class-cycle clauses asserted before the first check.
    pub preemptive_cycles: Option<PreemptiveCyclesConfig>,
    /// How cyclic models are refined.
    pub cycle_blocking: CycleBlocking,
    /// Fraction of the remaining gap to descend by on each probe.
    pub descent_fraction: f64,
}

impl Default for SatDescendingConfig {
    fn default() -> Self {
        Self {
            preemptive_cycles: Some(PreemptiveCyclesConfig::default()),
            cycle_blocking: CycleBlocking::default(),
            descent_fraction: 1.0 / 64.0,
        }
    }
}

/// Structural reduction mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterMode {
    /// Preserve at least one optimal solution.
    AtLeastOneOptimal,
    /// Remove dominated alternatives regardless of cost; preserve existence.
    AtLeastOneSolution,
    /// Structural rules plus the top-down component filter. Preserves at
    /// least one optimum on the pure components it analyzes. The private-cone
    /// rule is restricted to components without opaque inputs; components with
    /// `I != {}` get the cheaper alternative and cascade rules.
    ComponentOptimal,
    // The full four-level contract also has:
    // - `AllOptimal`: preserve every optimal solution.
    // - `BestEffort`: no guarantee.
    // They are not implemented/used yet.
}

/// Relative or absolute budget for a child node.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TimeoutPolicy {
    /// Use the parent's remaining budget.
    Remaining,
    /// Fraction of the parent's remaining budget.
    Fraction(f64),
    /// Fraction of the parent's remaining budget, capped absolutely.
    FractionCapped { fraction: f64, cap: Duration },
    /// At most this long from now, capped by the parent's remaining budget.
    Time(Duration),
}

/// One-shot leaf algorithms.
#[derive(Clone, Debug, PartialEq)]
pub enum AtomicAlgorithm {
    Naive,
    Chain(BestEffortConfig),
    MaxSat(MaxSatIncSccConfig),
    SatDescending(SatDescendingConfig),
}

/// Recursive pipeline description.
#[derive(Clone, Debug, PartialEq)]
pub enum Node {
    Atomic(AtomicAlgorithm),
    Filter(FilterMode),
    /// Run the child with a child budget.
    Timeout {
        policy: TimeoutPolicy,
        child: Box<Node>,
    },
    /// Run `primary`; stop on success, otherwise run `fallback`.
    Try {
        primary: Box<Node>,
        fallback: Box<Node>,
    },
    /// Run `primary`; on failure run `fallback`, then continue with the parent.
    OnFailure {
        primary: Box<Node>,
        fallback: Box<Node>,
    },
    /// Run all children in order, keeping the cheapest result.
    Bundle(Vec<Node>),
    Repeat(usize, Box<Node>),
    /// Repeat until no progress or a small round cap.
    Saturate(Box<Node>),
    /// Record a stage label for statistics.
    Label {
        name: &'static str,
        child: Box<Node>,
    },
}

/// The single solver configuration.
#[derive(Clone, Debug, PartialEq)]
pub struct SolverConfig {
    pub pipeline: Node,
    /// Hard wall-clock budget; `None` disables it.
    pub timeout: Option<Duration>,
    /// Stop once a solution exists and this budget is reached; `None` disables.
    pub soft_timeout: Option<Duration>,
    /// Optional display/stat name for the whole pipeline.
    pub name: Option<&'static str>,
}

/// An assignment over the full vertex id space; disabled vertices must be
/// false.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Assignment {
    pub selected: Vec<bool>,
}

impl Assignment {
    pub(crate) fn new(selected: Vec<bool>) -> Self {
        Self { selected }
    }
}

/// Known cost bounds for an imprecise result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bounds {
    /// Proven lower bound, if one is known.
    pub lower: Option<usize>,
    /// Cost of the returned assignment.
    pub upper: usize,
}

/// Solver outcome.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Proven optimal on the view it was found on.
    Optimal(Assignment),
    /// Valid but not proven optimal; bounds are for reference.
    Any {
        assignment: Assignment,
        bounds: Bounds,
    },
    /// Proven that no valid assignment exists on that view.
    Infeasible,
}
