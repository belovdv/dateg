mod error;
mod graph;
mod stats;

use std::{
    any::TypeId,
    collections::{BTreeMap, BTreeSet},
    marker::PhantomData,
    time::{Duration, Instant},
};

use ahash::AHashMap;
use dateg::*;

use crate::dag::graph::VertexId;

pub use dateg_extractors_macro::index_dag;
pub use error::ExtractError;
pub(crate) use error::{check_deadline, remaining};
pub use graph::{
    BestEffortAlgorithm, BestEffortConfig, CombinatorConfig, CombinatorConfigBuilder,
    CycleBlocking, Fallback, FilterMode, FirstResult, MaxSatIncSccConfig, SatIncSccConfig,
    SolverConfig,
};
pub(crate) use stats::StatsSink;
pub use stats::{ExtractStats, StatValue};

pub trait Constructor: 'static {
    type Inputs: TokenTuple;
    type Output: TokenOpaqueMarker;
    type Enum: Eq;
    fn into_variant(_: Self::Inputs) -> Self::Enum;
    type Index;
    fn cost(_: Self::Inputs, _: &dateg::EGraph) -> Option<usize>;
    fn consumes(_: Self::Inputs) -> impl TokenTuple {
        ()
    }
}

/// Typed projection of the selected constructors.
///
/// The backing map is an `AHashMap`; its iteration order is not part of the
/// determinism contract. Traverse from the root or use graph order instead.
pub trait IndexFor<Token: TokenOpaqueMarker> {
    type Enum: Copy + Eq;
    fn get_map(&self) -> &AHashMap<Token, Self::Enum>;
    fn get_map_mut(&mut self) -> &mut AHashMap<Token, Self::Enum>;

    fn value(&self, token: Token) -> Self::Enum {
        self.get_map()[&token]
    }
    fn insert(&mut self, token: Token, value: Self::Enum) {
        let was = self.get_map_mut().insert(token, value);
        assert!(
            was.is_none(),
            "multiple constructors were picked for the same value"
        );
    }
}

/// Extraction graph builder.
///
/// Construction is deterministic for a fixed egraph: callbacks run in
/// registration order and each callback sorts its rows by egglog `Value`.
/// Every iterated collection below is ordered. Solvers may rely on graph
/// node/edge/conflict-group order, but must make their own deterministic
/// choices (tie-breaking, no hash iteration) and must treat wall-clock
/// deadlines as nondeterministic. Callers must register constructors in a
/// deterministic order. This relies on stable `Value` allocation; the testkit
/// `Memory`/`Nat` enforce that with single-thread EGraph pools.
#[derive(Default)]
pub struct Extractor<Index> {
    _p: PhantomData<Index>,

    dag: graph::Graph,
    used: Vec<bool>,

    values: BTreeMap<Value, VertexId>,
    constructors: BTreeMap<(FunctionId, Vec<Value>), VertexId>,
    alternatives: BTreeMap<VertexId, Vec<VertexId>>,
    consumers: BTreeMap<VertexId, BTreeSet<VertexId>>,

    callbacks_init_v: Vec<(TypeId, Box<dyn Fn(&mut Self, &EGraph)>)>,
    callbacks_init_c: Vec<(TypeId, Box<dyn Fn(&mut Self, &EGraph)>)>,
    callbacks_collect: Vec<(TypeId, Box<dyn Fn(&Self, &EGraph, &mut Index)>)>,
}

fn register_callback<F>(callbacks: &mut Vec<(TypeId, F)>, tid: TypeId, callback: F) {
    if let Some((_, slot)) = callbacks.iter_mut().find(|(id, _)| *id == tid) {
        *slot = callback;
    } else {
        callbacks.push((tid, callback));
    }
}

pub struct ExtractOptions {
    solver: SolverConfig,
    timeout: Option<Duration>,
    validate: bool,
    filtration: Option<FilterMode>,
}

pub struct ExtractOptionsBuilder {
    solver: SolverConfig,
    timeout: Option<Duration>,
    validate: bool,
    filtration: Option<FilterMode>,
}

impl ExtractOptions {
    /// Default wall-clock budget. Disable explicitly with `timeout(None)`.
    pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10 * 60);

    /// Start building options for `solver`.
    ///
    /// Defaults: validation enabled, [`Self::DEFAULT_TIMEOUT`].
    pub fn builder(solver: SolverConfig) -> ExtractOptionsBuilder {
        ExtractOptionsBuilder {
            solver,
            timeout: Some(Self::DEFAULT_TIMEOUT),
            validate: true,
            filtration: None,
        }
    }
}

impl ExtractOptionsBuilder {
    /// Set the wall-clock budget; `None` disables it.
    pub fn timeout(mut self, timeout: Option<Duration>) -> Self {
        self.timeout = timeout;
        self
    }

    /// Enable or disable validation of the solver assignment.
    pub fn validate(mut self, validate: bool) -> Self {
        self.validate = validate;
        self
    }

    /// Reduce the solver graph before extraction. `None` leaves it unchanged.
    pub fn filtration(mut self, filtration: Option<FilterMode>) -> Self {
        self.filtration = filtration;
        self
    }

    pub fn build(self) -> ExtractOptions {
        ExtractOptions {
            solver: self.solver,
            timeout: self.timeout,
            validate: self.validate,
            filtration: self.filtration,
        }
    }
}

#[derive(Debug)]
pub struct Extraction<Index> {
    pub index: Index,
    pub cost: usize,
}

macro_rules! ensure {
    ($cond:expr, $err:expr) => {
        if !$cond {
            return Err($err);
        }
    };
}

impl<Index: Default> Extractor<Index> {
    pub fn extract(
        mut self,
        eg: &EGraph,
        root: impl TokenOpaqueMarker,
        options: &ExtractOptions,
        stats: Option<&mut ExtractStats>,
    ) -> Result<Extraction<Index>, ExtractError> {
        let started = Instant::now();
        let deadline = options.timeout.map(|timeout| started + timeout);
        let mut stats = StatsSink(stats);
        if let Some(stats) = stats.0.as_deref_mut() {
            *stats = ExtractStats::default();
        }

        let result = self.extract_inner(eg, root, options, deadline, &mut stats);
        stats.total(started.elapsed());
        result
    }

    fn extract_inner(
        &mut self,
        eg: &EGraph,
        root: impl TokenOpaqueMarker,
        options: &ExtractOptions,
        deadline: Option<Instant>,
        stats: &mut StatsSink<'_>,
    ) -> Result<Extraction<Index>, ExtractError> {
        // Deterministic construction contract: callbacks are taken in
        // registration order, and each callback sorts its rows by egglog
        // `Value`. The graph handed to the solver is therefore stable for a
        // fixed egraph. Assumes stable `Value` allocation; testkit
        // `Memory`/`Nat` enforce that with single-thread EGraph pools.
        for (_, init_v) in std::mem::take(&mut self.callbacks_init_v) {
            init_v(self, eg);
        }
        for (_, init_c) in std::mem::take(&mut self.callbacks_init_c) {
            init_c(self, eg);
        }
        for (value, alternatives) in std::mem::take(&mut self.alternatives) {
            self.dag.add_conflicting_group(alternatives.iter().copied());
            self.dag.add_edges(value, alternatives);
        }
        let mut consumption_edges = 0usize;
        let mut forked_values = 0usize;
        let mut max_consumers = 0usize;
        for consumed_by in self.consumers.values() {
            consumption_edges += consumed_by.len();
            if consumed_by.len() > 1 {
                forked_values += 1;
            }
            max_consumers = max_consumers.max(consumed_by.len());
            self.dag.add_conflicting_group(consumed_by.iter().copied());
        }
        stats.count("graph.consumed_values", self.consumers.len() as u64);
        stats.count("graph.consumption_edges", consumption_edges as u64);
        stats.count("graph.forked_values", forked_values as u64);
        stats.count("graph.max_consumers", max_consumers as u64);

        // TODO: the extractor could canonicalize here, but currently requires
        // the caller to do it.
        ensure!(
            root.as_token_opaque().canon(eg) == root.as_token_opaque(),
            ExtractError::RootNotCanonical
        );
        let root_vertex = *self
            .values
            .get(&root.into_egglog())
            .ok_or(ExtractError::RootMissing)?;
        self.dag.set_roots([root_vertex]);
        if let Some(mode) = options.filtration {
            let started = Instant::now();
            let report = graph::filter(&self.dag, mode);
            stats.duration("filtration.analyze", started.elapsed());
            let disabled = report.disabled.iter().filter(|&&flag| flag).count() as u64;
            stats.text(
                "filtration.mode",
                match mode {
                    FilterMode::Validity => "validity",
                    FilterMode::Optimal => "optimal",
                },
            );
            stats.count("filtration.alternatives", report.alternatives);
            stats.count("filtration.consumers", report.consumers);
            stats.count("filtration.removed_self", report.removed_self);
            stats.count("filtration.removed_duplicate", report.removed_duplicate);
            stats.count("filtration.removed_dominated", report.removed_dominated);
            stats.count("filtration.disabled", disabled);
            self.dag.set_disabled(report.disabled);
        }

        let solution = self.dag.solve_with(&options.solver, deadline, stats)?;

        if options.validate {
            let t = Instant::now();
            let valid = self.dag.validate(&solution.assignment);
            stats.verify(t.elapsed());
            ensure!(valid, ExtractError::InvalidSolution);
        }
        self.used = solution.assignment;

        let mut index = Index::default();
        for (_, collect) in std::mem::take(&mut self.callbacks_collect) {
            collect(&self, eg, &mut index);
        }

        Ok(Extraction {
            index,
            cost: solution.cost,
        })
    }

    pub fn set_constructor<C: Constructor, S>(&mut self, table: Table<S>)
    where
        Index: IndexFor<C::Output, Enum = C::Enum>,
        S: Schema<Inputs = C::Inputs, Output = C::Output, AllowAdd = True>,
    {
        self.set_constructor_ext::<C, S>(table, C::cost, |is| C::consumes(is).into_egglog_vec());
    }

    pub fn set_constructor_ext<C: Constructor, S>(
        &mut self,
        table: Table<S>,
        custom_cost: impl Fn(C::Inputs, &dateg::EGraph) -> Option<usize> + Send + Sync + 'static,
        custom_consumes: impl Fn(C::Inputs) -> Vec<Value> + Send + Sync + 'static,
    ) where
        Index: IndexFor<C::Output, Enum = C::Enum>,
        S: Schema<Inputs = C::Inputs, Output = C::Output, AllowAdd = True>,
    {
        let tid = TypeId::of::<C>();
        let fid = table.into_egglog();

        let init_v = move |ext: &mut Self, eg: &EGraph| {
            // Sort by `Value` so value vertex ids do not depend on table row
            // order.
            let mut outputs = Vec::new();
            eg.for_each_row(table, |_, output| outputs.push(output.into_egglog()));
            outputs.sort_unstable();
            outputs.dedup();
            for output in outputs {
                let add_v = || {
                    ext.dag
                        .add_vertex(false, 0, || Some(format!("v{output:?}")))
                };
                ext.values.entry(output).or_insert_with(add_v);
            }
        };
        register_callback(&mut self.callbacks_init_v, tid, Box::new(init_v));

        let init_c = move |ext: &mut Self, eg: &EGraph| {
            // Sort rows so constructor vertex and edge insertion order is
            // stable.
            let mut rows = Vec::new();
            eg.for_each_row(table, |inputs, output| {
                rows.push((inputs.into_egglog_vec(), inputs, output));
            });
            rows.sort_unstable_by(|a, b| {
                a.0.cmp(&b.0)
                    .then_with(|| a.2.into_egglog().cmp(&b.2.into_egglog()))
            });
            for (input_key, inputs, output_) in rows {
                let output = output_.into_egglog();
                let non_primitive: Vec<_> = inputs.into_non_primitive().collect();
                if non_primitive
                    .iter()
                    .any(|&val| val == output || !ext.values.contains_key(&val))
                {
                    continue;
                }
                let Some(cost) = custom_cost(inputs, eg) else {
                    continue;
                };
                let cstr = ext
                    .dag
                    .add_vertex(true, cost, || Some(format!("c{output_:?}")));
                let was = ext.constructors.insert((fid, input_key), cstr);
                assert!(was.is_none());
                ext.dag
                    .add_edges(cstr, non_primitive.iter().copied().map(|v| ext.values[&v]));
                let value = ext.values[&output];
                ext.alternatives.entry(value).or_default().push(cstr);
                let consumed: Vec<VertexId> = custom_consumes(inputs)
                    .into_iter()
                    .map(|value| ext.values[&value])
                    .collect();
                ext.dag.set_consumes(cstr, consumed.iter().copied());
                for &consumed in &consumed {
                    ext.consumers.entry(consumed).or_default().insert(cstr);
                }
            }
        };
        register_callback(&mut self.callbacks_init_c, tid, Box::new(init_c));

        let collect = move |ext: &Self, eg: &EGraph, index: &mut Index| {
            let mut rows = Vec::new();
            eg.for_each_row(table, |inputs, output| {
                rows.push((inputs.into_egglog_vec(), inputs, output));
            });
            rows.sort_unstable_by(|a, b| {
                a.0.cmp(&b.0)
                    .then_with(|| a.2.into_egglog().cmp(&b.2.into_egglog()))
            });
            for (input_key, inputs, output) in rows {
                let Some(&cstr) = ext.constructors.get(&(fid, input_key)) else {
                    continue;
                };
                if ext.used[cstr] {
                    index.insert(output, C::into_variant(inputs));
                }
            }
        };
        register_callback(&mut self.callbacks_collect, tid, Box::new(collect));
    }

    pub fn set_container<C: ContainerValueExt>(&mut self) {
        let tid = TypeId::of::<C>();

        let init_v = move |ext: &mut Self, eg: &EGraph| {
            // Container registry iteration is not ordered; sort the values
            // before assigning vertex ids.
            let mut outputs = Vec::new();
            eg._inner()
                .container_values()
                .for_each::<C>(|_, value| outputs.push(value));
            outputs.sort_unstable();
            outputs.dedup();
            for value in outputs {
                let add_v = || ext.dag.add_vertex(false, 0, || Some(format!("vContainer")));
                ext.values.entry(value).or_insert_with(add_v);
            }
        };
        register_callback(&mut self.callbacks_init_v, tid, Box::new(init_v));

        let init_c = move |ext: &mut Self, eg: &EGraph| {
            // Sort container rows before creating constructor vertices/edges.
            let mut rows = Vec::new();
            eg._inner()
                .container_values()
                .for_each::<C>(|inputs, output| {
                    rows.push((ContainerValue::iter(inputs).collect::<Vec<_>>(), output));
                });
            rows.sort_unstable_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
            for (inputs, output) in rows {
                if inputs.contains(&output) {
                    continue;
                }
                let cont = ext.dag.add_vertex(true, 0, || Some(format!("cContainer")));
                ext.dag
                    .add_edges(cont, inputs.iter().map(|value| ext.values[value]));
                let value = ext.values[&output];
                ext.alternatives.entry(value).or_default().push(cont);
            }
        };
        register_callback(&mut self.callbacks_init_c, tid, Box::new(init_c));
    }
}
