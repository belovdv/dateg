mod graph;

use std::{any::TypeId, marker::PhantomData};

use ahash::{AHashMap, AHashSet};
use dateg::*;

use crate::dag::graph::VertexId;

pub use dateg_extractors_macro::index_dag;

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

pub trait IndexFor<Token: TokenOpaqueMarker> {
    type Enum: Copy + Eq;
    fn get_map(&self) -> &AHashMap<Token, Self::Enum>;
    fn get_map_mut(&mut self) -> &mut AHashMap<Token, Self::Enum>;

    fn value(&self, token: Token) -> Self::Enum {
        self.get_map()[&token]
    }
    fn insert(&mut self, token: Token, value: Self::Enum) {
        let was = self.get_map_mut().insert(token, value);
        assert!(was.is_none());
    }
}

#[derive(Default)]
pub struct Extractor<Index> {
    _p: PhantomData<Index>,

    dag: graph::Graph,
    used: Vec<bool>,

    values: AHashMap<Value, VertexId>,
    constructors: AHashMap<(FunctionId, Vec<Value>), VertexId>,
    consumers: AHashMap<VertexId, AHashSet<VertexId>>,

    callbacks_init_v: AHashMap<TypeId, Box<dyn Fn(&mut Self, &EGraph)>>,
    callbacks_init_c: AHashMap<TypeId, Box<dyn Fn(&mut Self, &EGraph)>>,
    callbacks_collect: AHashMap<TypeId, Box<dyn Fn(&Self, &EGraph, &mut Index)>>,
}

impl<Index: Default> Extractor<Index> {
    pub fn extract(mut self, eg: &EGraph, root: impl TokenOpaqueMarker) -> Index {
        for init_v in std::mem::take(&mut self.callbacks_init_v).values() {
            (init_v)(&mut self, eg);
        }
        for init_c in std::mem::take(&mut self.callbacks_init_c).values() {
            (init_c)(&mut self, eg);
        }
        for consumed_by in self.consumers.values() {
            self.dag.add_conflicting_group(consumed_by.iter().copied());
        }
        assert!(
            root.as_token_opaque().canon(eg) == root.as_token_opaque(),
            "please canonicalize root token"
        );
        self.dag.set_roots([self.values[&root.into_egglog()]]);
        self.used = self.dag.solve(graph::SolverConfig::MaxSat {});
        let mut index = Index::default();
        for collect in std::mem::take(&mut self.callbacks_collect).values() {
            (collect)(&self, eg, &mut index);
        }
        index
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
            eg.for_each_row(table, |_, output| {
                let add_v = || {
                    ext.dag
                        .add_vertex(false, 0, || Some(format!("v{output:?}")))
                };
                ext.values.entry(output.into_egglog()).or_insert_with(add_v);
            });
        };
        self.callbacks_init_v.insert(tid, Box::new(init_v));

        let init_c = move |ext: &mut Self, eg: &EGraph| {
            let mut options: AHashMap<VertexId, Vec<VertexId>> = Default::default();
            eg.for_each_row(table, |inputs, output_| {
                let output = output_.into_egglog();
                if inputs
                    .into_non_primitive()
                    .any(|val| val == output || !ext.values.contains_key(&val))
                {
                    return;
                }
                let Some(cost) = custom_cost(inputs, eg) else {
                    return;
                };
                let cstr = ext
                    .dag
                    .add_vertex(true, cost, || Some(format!("c{output_:?}")));
                let was = ext
                    .constructors
                    .insert((fid, inputs.into_egglog_vec()), cstr);
                assert!(was.is_none());
                ext.dag
                    .add_edges(cstr, inputs.into_non_primitive().map(|v| ext.values[&v]));
                options.entry(ext.values[&output]).or_default().push(cstr);
                for consumed in custom_consumes(inputs)
                    .into_iter()
                    .map(|value| ext.values[&value])
                {
                    ext.consumers.entry(consumed).or_default().insert(cstr);
                }
            });
            for (value, constructors) in options {
                ext.dag.add_edges(value, constructors);
            }
        };
        self.callbacks_init_c.insert(tid, Box::new(init_c));

        let collect = move |ext: &Self, eg: &EGraph, index: &mut Index| {
            eg.for_each_row(table, |inputs, output| {
                let Some(&cstr) = ext.constructors.get(&(fid, inputs.into_egglog_vec())) else {
                    return;
                };
                if ext.used[cstr] {
                    index.insert(output, C::into_variant(inputs));
                }
            });
        };
        self.callbacks_collect.insert(tid, Box::new(collect));
    }

    pub fn set_container<C: ContainerValueExt>(&mut self) {
        let tid = TypeId::of::<C>();

        let init_v = move |ext: &mut Self, eg: &EGraph| {
            eg._inner().container_values().for_each::<C>(|_, v| {
                let add_v = || ext.dag.add_vertex(false, 0, || Some(format!("vContainer")));
                ext.values.entry(v).or_insert_with(add_v);
            });
        };
        self.callbacks_init_v.insert(tid, Box::new(init_v));

        let init_c = move |ext: &mut Self, eg: &EGraph| {
            let mut options: AHashMap<VertexId, Vec<VertexId>> = Default::default();
            eg._inner()
                .container_values()
                .for_each::<C>(|inputs, output| {
                    if ContainerValue::iter(inputs).any(|input| input == output) {
                        return;
                    }
                    let cont = ext.dag.add_vertex(true, 0, || Some(format!("cContainer")));
                    ext.dag
                        .add_edges(cont, ContainerValue::iter(inputs).map(|v| ext.values[&v]));
                    options.entry(ext.values[&output]).or_default().push(cont);
                });
            for (value, container) in options {
                ext.dag.add_edges(value, container);
            }
        };
        self.callbacks_init_c.insert(tid, Box::new(init_c));
    }
}
