mod macros;
mod rule_builder;
mod rule_run;
mod schema;
mod table;
mod token;
mod tuple;

use std::any::TypeId;

use ahash::AHashMap;

pub use rule_builder::*;
pub use schema::*;
pub use table::*;
pub use token::*;
pub use tuple::*;

pub use egglog_bridge::{FunctionId, RuleId};
pub use egglog_core_relations::{BaseValue, Value};

pub use dateg_macro::rule;

#[derive(Default)]
pub struct EGraph {
    inner: egglog_bridge::EGraph,
    tables: AHashMap<String, (TypeId, egglog_bridge::FunctionId)>,
    ruleset_active: String,
    pub rulesets: AHashMap<String, Vec<RuleId>>,
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

    pub fn set_ruleset_active(&mut self, rs: impl ToString) {
        self.ruleset_active = rs.to_string();
    }
    pub fn add_ruleset_rule(&mut self, rule: RuleId) {
        self.rulesets
            .entry(self.ruleset_active.clone())
            .or_default()
            .push(rule);
    }

    pub fn _inner(&self) -> &egglog_bridge::EGraph {
        &self.inner
    }
    pub fn _inner_mut(&mut self) -> &mut egglog_bridge::EGraph {
        &mut self.inner
    }
}
