mod access;
mod function;
mod macros;
mod rule_build;
mod rule_run;
mod schema;
mod table;
mod token;
mod utils;

use std::any::TypeId;

use ahash::AHashMap;

pub use function::*;
pub use rule_build::*;
pub use schema::*;
pub use table::*;
pub use token::*;
pub use utils::*;

pub use egglog_bridge::{FunctionId, RuleId};
pub use egglog_core_relations::{BaseValue, ContainerValue, Value, ValueRebuilder};

pub use dateg_macro::rule;

#[derive(Default, Clone)]
pub struct EGraph {
    inner: egglog_bridge::EGraph,
    tables: AHashMap<String, (TypeId, egglog_bridge::FunctionId)>,
    pub ruleset_active: String,
    pub rulesets: AHashMap<String, Vec<RuleId>>,
}

impl EGraph {
    pub fn add_primitive_type<T: BaseValue>(&mut self) {
        self.inner.base_values_mut().register_type::<T>();
    }
    pub fn add_primitive_value<V: BaseValue>(&mut self, value: V) -> TokenPrimitive<V> {
        TokenPrimitive::from_egglog(self.inner.base_values_mut().get(value))
    }

    pub fn add_container_type<C: ContainerValue>(&mut self) {
        self.inner.register_container_ty::<C>();
    }
    pub fn add_container_value<C: ContainerValueExt>(&mut self, c: C) -> TokenContainer<C> {
        TokenContainer::from_egglog(self.inner.get_container_value(c))
    }

    pub fn _inner(&self) -> &egglog_bridge::EGraph {
        &self.inner
    }
    pub fn _inner_mut(&mut self) -> &mut egglog_bridge::EGraph {
        &mut self.inner
    }
}
