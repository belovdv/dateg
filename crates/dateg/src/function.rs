use std::marker::PhantomData;

use egglog_core_relations::{ExecutionState, ExternalFunctionId, make_external_func};

use crate::*;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Function<S: Schema>(ExternalFunctionId, PhantomData<S>);

impl<S: Schema> Function<S> {
    pub fn from_egglog(id: ExternalFunctionId) -> Self {
        Self(id, PhantomData)
    }
    pub fn into_egglog(self) -> ExternalFunctionId {
        self.0
    }
}

impl EGraph {
    pub fn new_function<Inputs: TokenTuplePartialResolve, Output: FunctionReturnValue>(
        &mut self,
        f: impl Fn(Inputs::Resolved) -> Output::Interface + Clone + Send + Sync + 'static,
    ) -> Function<(Inputs, Output, True)> {
        self.new_function_partial(move |args| Some(f(args)))
    }

    pub fn new_function_partial<Inputs: TokenTuplePartialResolve, Output: FunctionReturnValue>(
        &mut self,
        f: impl Fn(Inputs::Resolved) -> Option<Output::Interface> + Clone + Send + Sync + 'static,
    ) -> Function<(Inputs, Output, True)> {
        let f = make_external_func(move |es, values| {
            let r = f(Inputs::from_egglog(values).partial_resolve(es));
            <Output as FunctionReturnValue>::into_value(es, r)
        });
        let id = self.inner.register_external_func(Box::new(f));
        Function(id, PhantomData)
    }
}

pub trait FunctionReturnValue: Token {
    type Interface;
    fn into_value(es: &mut ExecutionState, v: Option<Self::Interface>) -> Option<Value>;
}
impl<T: BaseValue> FunctionReturnValue for TokenPrimitive<T> {
    type Interface = T;
    fn into_value(es: &mut ExecutionState, v: Option<Self::Interface>) -> Option<Value> {
        v.map(|v| es.base_values().get::<T>(v))
    }
}
impl<T: Send + Sync + 'static> FunctionReturnValue for TokenOpaque<T> {
    type Interface = Self;
    fn into_value(_: &mut ExecutionState, v: Option<Self::Interface>) -> Option<Value> {
        v.map(|v| v.into_egglog())
    }
}
impl<T: ContainerValueExt> FunctionReturnValue for TokenContainer<T> {
    type Interface = T;
    fn into_value(es: &mut ExecutionState, v: Option<Self::Interface>) -> Option<Value> {
        Some(es.clone().container_values().register_val(v?, es))
    }
}
