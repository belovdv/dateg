mod macros;
mod schema;
mod table;
mod token;
mod tuple;

pub use schema::*;
pub use table::*;
pub use token::*;
pub use tuple::*;

#[derive(Default)]
pub struct EGraph {
    inner: egglog_bridge::EGraph,
}

impl EGraph {
    pub fn add_primitive_type<T: egglog_core_relations::BaseValue>(&mut self) {
        self.inner.base_values_mut().register_type::<T>();
    }

    pub fn add_primitive_value<V: egglog_core_relations::BaseValue>(
        &mut self,
        value: V,
    ) -> TokenValuePrimitive<V> {
        TokenValuePrimitive::from_value(self.inner.base_values_mut().get(value))
    }

    pub fn _inner(&self) -> &egglog_bridge::EGraph {
        &self.inner
    }
    pub fn _inner_mut(&mut self) -> &mut egglog_bridge::EGraph {
        &mut self.inner
    }
}
