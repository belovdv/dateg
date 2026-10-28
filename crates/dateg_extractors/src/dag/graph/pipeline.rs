//! Pipeline evaluation.

use std::time::{Duration, Instant};

use super::{
    Assignment, AtomicAlgorithm, Bounds, FilterMode, Graph, Node, Outcome, SolverConfig,
    TimeoutPolicy, VertexId, View, assignment_cost, best_effort, filter_with_roots,
    max_sat_inc_scc, naive, record_metrics, sat_descending,
};
use crate::dag::{Error, StatsSink};

struct Run {
    deadline: Option<Instant>,
    soft: Option<Instant>,
}

impl Run {
    fn new(deadline: Option<Instant>, soft: Option<Instant>) -> Self {
        Self { deadline, soft }
    }

    fn child(&self, policy: TimeoutPolicy) -> Self {
        let now = Instant::now();
        let parent = self.deadline;
        let deadline = match policy {
            TimeoutPolicy::Remaining => parent,
            TimeoutPolicy::Fraction(fraction) => parent.map(|deadline| {
                let remaining = deadline.saturating_duration_since(now);
                (now + scaled(remaining, fraction)).min(deadline)
            }),
            TimeoutPolicy::FractionCapped { fraction, cap } => {
                let capped = now + cap;
                match parent {
                    Some(deadline) => {
                        let remaining = deadline.saturating_duration_since(now);
                        Some(
                            (now + scaled(remaining, fraction))
                                .min(capped)
                                .min(deadline),
                        )
                    }
                    None => Some(capped),
                }
            }
            TimeoutPolicy::Time(time) => {
                let own = now + time;
                Some(parent.map_or(own, |deadline| own.min(deadline)))
            }
        };
        Self {
            deadline,
            soft: self.soft,
        }
    }

    fn deadline(&self) -> Option<Instant> {
        self.deadline
    }

    fn soft(&self) -> Option<Instant> {
        self.soft
    }

    fn soft_reached(&self) -> bool {
        self.soft.is_some_and(|soft| Instant::now() >= soft)
    }
}

fn scaled(duration: Duration, fraction: f64) -> Duration {
    if fraction <= 0.0 || !fraction.is_finite() {
        return Duration::ZERO;
    }
    Duration::from_secs_f64(duration.as_secs_f64() * fraction)
}

enum Status {
    Solution,
    Infeasible,
    Empty,
    Failed(Error),
}

struct Context<'a, 's> {
    graph: &'a Graph,
    roots: &'a [VertexId],
    view: View,
    stats: &'a mut StatsSink<'s>,
    best: Option<Assignment>,
    best_lower: Option<usize>,
    best_cost: usize,
    optimal: bool,
    stage: &'static str,
    last_error: Option<Error>,
    stop: bool,
    /// A reduction that is not existence-preserving has run; a later
    /// `Infeasible` must not be returned as a proof.
    lossy: bool,
}

pub(crate) fn solve(
    graph: &Graph,
    roots: &[VertexId],
    base_enabled: Vec<bool>,
    config: &SolverConfig,
    deadline: Option<Instant>,
    soft_deadline: Option<Instant>,
    stats: &mut StatsSink<'_>,
) -> Result<Outcome, Error> {
    let mut context = Context {
        graph,
        roots,
        view: View::new(graph, base_enabled, Vec::new()),
        stats,
        best: None,
        best_lower: None,
        best_cost: usize::MAX,
        optimal: false,
        stage: "pipeline",
        last_error: None,
        stop: false,
        lossy: false,
    };

    let run = Run::new(deadline, soft_deadline);
    let solve_started = Instant::now();
    let status = context.eval(&config.pipeline, &run);
    context.stats.solver(solve_started.elapsed());

    if let Some(name) = config.name {
        context.stats.text("solver.name", name);
    }
    if let Some(best) = context.best {
        return Ok(if context.optimal {
            Outcome::Optimal(best)
        } else {
            Outcome::Any {
                assignment: best,
                bounds: Bounds {
                    lower: context.best_lower,
                    upper: context.best_cost,
                },
            }
        });
    }
    match status {
        Status::Infeasible if context.lossy => Err(Error::NotFound),
        Status::Infeasible => Ok(Outcome::Infeasible),
        Status::Failed(error) => Err(error),
        Status::Empty => Err(context.last_error.unwrap_or(Error::NotFound)),
        Status::Solution => Err(Error::NotFound),
    }
}

impl Context<'_, '_> {
    fn cost(&self, assignment: &Assignment) -> usize {
        assignment_cost(self.graph, &self.view, &assignment.selected)
    }

    fn offer(&mut self, outcome: Outcome) {
        match outcome {
            Outcome::Optimal(assignment) => {
                let cost = self.cost(&assignment);
                if cost < self.best_cost || (cost == self.best_cost && !self.optimal) {
                    self.best = Some(assignment);
                    self.best_cost = cost;
                    self.best_lower = Some(cost);
                    self.optimal = true;
                    if self.stage != "pipeline" {
                        self.stats.text("combinator.stage", self.stage);
                    }
                }
            }
            Outcome::Any { assignment, bounds } => {
                let cost = self.cost(&assignment);
                if let Some(lower) = bounds.lower {
                    if lower <= self.best_cost {
                        self.best_lower = Some(match self.best_lower {
                            Some(current) => current.max(lower),
                            None => lower,
                        });
                    }
                }
                if cost < self.best_cost || (cost == self.best_cost && !self.optimal) {
                    self.best = Some(assignment);
                    self.best_cost = cost;
                    self.optimal = false;
                    if self.stage != "pipeline" {
                        self.stats.text("combinator.stage", self.stage);
                    }
                }
            }
            Outcome::Infeasible => {}
        }
    }

    fn solve_leaf(&mut self, algorithm: &AtomicAlgorithm, run: &Run) -> Status {
        record_metrics(self.graph, self.roots, &self.view, self.stats);
        let soft = run.soft().filter(|_| self.best.is_some());
        let result = match algorithm {
            AtomicAlgorithm::Naive => {
                self.stats.text("solver.name", "naive");
                naive::solve(self.graph, self.roots, &self.view, run.deadline(), soft).map(
                    |(_, assignment)| {
                        self.offer(Outcome::Optimal(Assignment::new(assignment)));
                        Status::Solution
                    },
                )
            }
            AtomicAlgorithm::Chain(config) => {
                self.stats.text("solver.name", "chain");
                best_effort::solve(
                    self.graph,
                    self.roots,
                    &self.view,
                    config,
                    run.deadline(),
                    soft,
                    self.stats,
                )
                .map(|assignment| {
                    let cost = self.cost(&Assignment::new(assignment.clone()));
                    self.offer(Outcome::Any {
                        assignment: Assignment::new(assignment),
                        bounds: Bounds {
                            lower: None,
                            upper: cost,
                        },
                    });
                    Status::Solution
                })
            }
            AtomicAlgorithm::MaxSat(config) => {
                self.stats.text("solver.name", "maxsat.inc_scc");
                (|| {
                    let mut solver = max_sat_inc_scc::MaxSatIncSccExtractor::new(
                        self.graph,
                        self.roots,
                        &self.view,
                        config,
                        run.deadline(),
                        soft,
                    )?;
                    solver.solve(run.deadline(), self.stats)
                })()
                .map(|assignment| {
                    self.offer(Outcome::Optimal(Assignment::new(assignment)));
                    Status::Solution
                })
            }
            AtomicAlgorithm::SatDescending(config) => {
                self.stats.text("solver.name", "sat.descending");
                let hint = self.best.as_ref().map(|best| best.selected.clone());
                (|| {
                    let mut solver = sat_descending::SatDescendingExtractor::new(
                        self.graph,
                        self.roots,
                        &self.view,
                        config,
                        run.deadline(),
                        run.soft(),
                        hint,
                    )?;
                    solver.solve(run.deadline(), self.stats)
                })()
                .map(|result| {
                    let assignment = Assignment::new(result.assignment);
                    if result.optimal {
                        self.offer(Outcome::Optimal(assignment));
                    } else {
                        let cost = self.cost(&assignment);
                        self.offer(Outcome::Any {
                            assignment,
                            bounds: Bounds {
                                lower: result.lower_bound,
                                upper: cost,
                            },
                        });
                    }
                    Status::Solution
                })
            }
        };
        match result {
            Ok(status) => status,
            Err(Error::Infeasible) => Status::Infeasible,
            Err(error) => {
                self.last_error = Some(match &error {
                    Error::Timeout => Error::Timeout,
                    Error::NotFound => Error::NotFound,
                    _ => Error::Custom(error.to_string()),
                });
                Status::Failed(error)
            }
        }
    }

    fn eval(&mut self, node: &Node, run: &Run) -> Status {
        if self.best.is_some() && run.soft_reached() {
            self.stop = true;
            return Status::Empty;
        }
        match node {
            Node::Atomic(algorithm) => self.solve_leaf(algorithm, run),
            Node::Filter(mode) => {
                if self.apply_filter(*mode, run.deadline()) {
                    Status::Infeasible
                } else {
                    Status::Empty
                }
            }
            Node::Timeout { policy, child } => {
                let child_run = run.child(*policy);
                self.eval(child, &child_run)
            }
            Node::Try { primary, fallback } => {
                let status = self.eval(primary, run);
                if self.stop || matches!(status, Status::Solution | Status::Infeasible) {
                    if self.best.is_some() {
                        Status::Solution
                    } else {
                        status
                    }
                } else {
                    self.eval(fallback, run)
                }
            }
            Node::OnFailure { primary, fallback } => {
                let status = self.eval(primary, run);
                if self.stop || matches!(status, Status::Solution | Status::Infeasible) {
                    if self.best.is_some() {
                        Status::Solution
                    } else {
                        status
                    }
                } else {
                    match self.eval(fallback, run) {
                        Status::Failed(error) => {
                            self.last_error = Some(error);
                            Status::Empty
                        }
                        status => status,
                    }
                }
            }
            Node::Bundle(nodes) => self.eval_bundle(nodes, run),
            Node::Repeat(times, child) => {
                let mut status = Status::Empty;
                for _ in 0..*times {
                    if self.optimal || self.stop {
                        break;
                    }
                    status = self.eval(child, run);
                }
                status
            }
            Node::Saturate(child) => {
                let mut status = Status::Empty;
                for _ in 0..8 {
                    if self.stop {
                        break;
                    }
                    let before_revision = self.view_revision();
                    let before_cost = self.best_cost;
                    status = self.eval(child, run);
                    if self.optimal
                        || (before_revision == self.view_revision()
                            && before_cost == self.best_cost)
                    {
                        break;
                    }
                }
                status
            }
            Node::Label { name, child } => {
                let previous = self.stage;
                self.stage = *name;
                let started = Instant::now();
                let status = self.eval(child, run);
                if self.stats.enabled() {
                    self.stats
                        .duration(&format!("combinator.{name}"), started.elapsed());
                    if let Status::Failed(error) = &status {
                        self.stats.text(
                            &format!("combinator.{name}_error"),
                            error.to_string().replace(' ', "_"),
                        );
                    }
                }
                self.stage = previous;
                status
            }
        }
    }

    fn eval_bundle(&mut self, nodes: &[Node], run: &Run) -> Status {
        let mut solution = false;
        let mut infeasible = false;
        let mut failed = None;
        for node in nodes {
            if self.optimal || self.stop {
                break;
            }
            match self.eval(node, run) {
                Status::Solution => solution = true,
                Status::Infeasible => {
                    infeasible = true;
                    break;
                }
                Status::Empty => {}
                Status::Failed(error) => failed = Some(error),
            }
        }
        if solution {
            Status::Solution
        } else if infeasible {
            Status::Infeasible
        } else if let Some(error) = failed {
            Status::Failed(error)
        } else {
            Status::Empty
        }
    }

    fn view_revision(&self) -> usize {
        self.view
            .enabled
            .iter()
            .filter(|&&enabled| !enabled)
            .count()
            + self.view.extra_groups.len()
    }

    fn apply_filter(&mut self, mode: FilterMode, deadline: Option<Instant>) -> bool {
        let report = filter_with_roots(self.graph, mode, &self.view.enabled, self.roots, deadline);
        let disabled = report.disabled.iter().filter(|&&disabled| disabled).count() as u64;
        for (index, &disabled) in report.disabled.iter().enumerate() {
            if disabled && self.view.enabled[index] {
                self.view.enabled[index] = false;
            }
        }
        self.stats.count("filter.disabled", disabled);
        self.stats.text(
            "filter.mode",
            match mode {
                FilterMode::AtLeastOneOptimal => "optimal",
                FilterMode::AtLeastOneSolution => "validity",
                FilterMode::ComponentOptimal => "component",
                FilterMode::DeadVertices => "dead",
                FilterMode::ComponentBottomUp => "component_bottom_up",
                FilterMode::ComponentBottomUpFrontier(_) => "component_bottom_up_frontier",
            },
        );
        if let Some(dead) = &report.dead {
            self.stats
                .count("filter.dead.unreachable", dead.removed_unreachable as u64);
            self.stats
                .count("filter.dead.unproducible", dead.removed_unproducible as u64);
            self.stats
                .count("filter.dead.root_dead", u64::from(dead.root_dead));
            self.stats
                .count("filter.dead.timed_out", u64::from(dead.timed_out));
            self.stats.duration("filter.dead.duration", dead.duration);
        }
        if let Some(bottom_up) = &report.bottom_up {
            self.stats
                .count("filter.bottom_up.components", bottom_up.components as u64);
            self.stats
                .count("filter.bottom_up.vertices", bottom_up.vertices as u64);
            self.stats
                .count("filter.bottom_up.needed", bottom_up.needed as u64);
            self.stats.count(
                "filter.bottom_up.recovered_values",
                bottom_up.recovered_values as u64,
            );
            self.stats.count(
                "filter.bottom_up.recovered_constructors",
                bottom_up.recovered_constructors as u64,
            );
            self.stats.count(
                "filter.bottom_up.collapsed_components",
                bottom_up.collapsed_components as u64,
            );
            self.stats
                .count("filter.bottom_up.missing", bottom_up.missing as u64);
            self.stats
                .count("filter.bottom_up.disabled", bottom_up.disabled_total as u64);
            self.stats
                .duration("filter.bottom_up.duration", bottom_up.duration);
        }
        if report.lossy {
            self.lossy = true;
            self.stats.count("filter.lossy", 1);
        }
        if let Some(frontier) = &report.frontier {
            self.stats
                .count("filter.frontier.components", frontier.components as u64);
            self.stats
                .count("filter.frontier.vertices", frontier.vertices as u64);
            self.stats
                .count("filter.frontier.processed", frontier.processed as u64);
            self.stats
                .count("filter.frontier.skipped_wide", frontier.skipped_wide as u64);
            self.stats.count(
                "filter.frontier.skipped_missing",
                frontier.skipped_missing as u64,
            );
            self.stats
                .count("filter.frontier.timed_out", u64::from(frontier.timed_out));
            self.stats
                .count("filter.frontier.supports", frontier.supports as u64);
            self.stats
                .count("filter.frontier.max_supports", frontier.max_frontier as u64);
            self.stats
                .count("filter.frontier.routes", frontier.routes as u64);
            self.stats
                .count("filter.frontier.disabled", frontier.disabled_total as u64);
            self.stats
                .duration("filter.frontier.duration", frontier.duration);
        }
        if report.infeasible {
            self.stats.count("filter.infeasible", 1);
        }
        if let Some(component) = &report.component {
            self.stats
                .count("component_filter.components", component.components as u64);
            self.stats
                .count("component_filter.vertices", component.vertices as u64);
            self.stats
                .count("component_filter.inputs", component.inputs as u64);
            self.stats
                .count("component_filter.outputs", component.outputs as u64);
            self.stats
                .count("component_filter.removed_a", component.removed_a as u64);
            self.stats
                .count("component_filter.removed_b", component.removed_b as u64);
            self.stats.count(
                "component_filter.removed_cascade",
                component.removed_cascade as u64,
            );
            self.stats
                .count("component_filter.rounds", component.rounds as u64);
            self.stats
                .duration("component_filter.analyze", component.duration);
            if component.timed_out {
                self.stats.count("component_filter.timed_out", 1);
            }
        }
        self.view.refresh(self.graph);
        report.infeasible
    }
}
