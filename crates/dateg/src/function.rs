use std::marker::PhantomData;

use egglog_core_relations::{ExternalFunctionId, make_external_func};

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
    pub fn new_function<Inputs: TokenTuplePrimitive, Output: TokenPrimitiveMarker>(
        &mut self,
        f: impl Fn(Inputs::Inner) -> Output::Inner + Clone + Send + Sync + 'static,
    ) -> Function<(Inputs, Output, True)> {
        self.new_function_partial(move |args| Some(f(args)))
    }

    pub fn new_function_partial<Inputs: TokenTuplePrimitive, Output: TokenPrimitiveMarker>(
        &mut self,
        f: impl Fn(Inputs::Inner) -> Option<Output::Inner> + Clone + Send + Sync + 'static,
    ) -> Function<(Inputs, Output, True)> {
        let f = make_external_func(move |es, values| {
            let r = f(Inputs::from_egglog(values).into_values(es));
            r.map(|r| es.base_values().get::<Output::Inner>(r))
        });
        let id = self.inner.register_external_func(Box::new(f));
        Function(id, PhantomData)
    }
}
