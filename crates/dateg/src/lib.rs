mod macros;
mod table;
mod token;

use egglog_core_relations::BaseValue;

pub use table::{Loader, TableSchema, TableToken};
pub use token::{Token, TokenValueOpaque, TokenValuePrimitive};

#[derive(Default)]
pub struct EGraph {
    inner: egglog_bridge::EGraph,
}

impl EGraph {
    pub fn add_primitive_type<T: BaseValue>(&mut self) {
        self.inner.base_values_mut().register_type::<T>();
    }

    pub fn add_primitive_value<V: BaseValue>(&mut self, value: V) -> TokenValuePrimitive<V> {
        TokenValuePrimitive::from_value(self.inner.base_values_mut().get(value))
    }
}
