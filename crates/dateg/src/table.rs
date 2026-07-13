use std::{any::TypeId, marker::PhantomData};

use egglog_bridge::{DefaultVal, FunctionConfig, FunctionId, MergeFn};
use egglog_core_relations::Value;

use crate::*;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Table<S: Schema>(FunctionId, PhantomData<S>);

impl<S: Schema> Table<S> {
    pub fn from_egglog(id: FunctionId) -> Self {
        Self(id, PhantomData)
    }
    pub fn into_egglog(self) -> FunctionId {
        self.0
    }
}

impl EGraph {
    fn new_table<S: Schema>(
        &mut self,
        name: impl ToString,
        default: DefaultVal,
        merge: MergeFn,
    ) -> Table<S> {
        let id = self.inner.add_table(FunctionConfig {
            schema: S::egglog(&self.inner),
            default,
            merge,
            name: name.to_string(),
            can_subsume: false,
        });
        let tid = TypeId::of::<S>();
        let was = self.tables.insert(name.to_string(), (tid, id));
        assert!(was.is_none());
        Table::from_egglog(id)
    }
    pub fn get_table<S: Schema>(&self, name: &str) -> Table<S> {
        let (tid, id) = self.tables[name];
        assert_eq!(tid, TypeId::of::<S>(), "{name}");
        Table::from_egglog(id)
    }

    pub fn new_table_constructor<Inputs: TokenTuple, Output: TokenOpaqueMarker>(
        &mut self,
        name: impl ToString,
    ) -> Table<(Inputs, Output, True)> {
        self.new_table(name, DefaultVal::FreshId, MergeFn::UnionId)
    }
    pub fn new_table_function<Inputs: TokenTuple, Output: Token>(
        &mut self,
        name: impl ToString,
    ) -> Table<(Inputs, Output, False)> {
        self.new_table(name, DefaultVal::Fail, MergeFn::AssertEq)
    }
    pub fn new_table_relation<Inputs: TokenTuple>(
        &mut self,
        name: impl ToString,
    ) -> Table<(Inputs, TokenPrimitive<()>, True)> {
        let v0 = DefaultVal::Const(Value::new_const(0));
        self.new_table(name, v0, MergeFn::AssertEq)
    }
    /// Merge: old, new -> merged
    pub fn new_table_function_with_merge<Inputs: TokenTuple, Output: Token>(
        &mut self,
        name: impl ToString,
        merge: Function<((Output, Output), Output, True)>,
    ) -> Table<(Inputs, Output, False)> {
        self.new_table(
            name,
            DefaultVal::Fail,
            MergeFn::Primitive(merge.into_egglog(), vec![MergeFn::Old, MergeFn::New]),
        )
    }

    pub fn for_each_row<S: Schema>(
        &self,
        table: Table<S>,
        mut f: impl FnMut(S::Inputs, S::Output),
    ) {
        self.inner.for_each(table.0, |entry| {
            let (output, inputs) = entry.vals.split_last().unwrap();
            let inputs = S::Inputs::from_egglog(inputs);
            let output = S::Output::from_egglog(*output);
            f(inputs, output);
        });
    }
    pub fn for_each_row_untyped<S: Schema>(&self, table: Table<S>, mut f: impl FnMut(&[Value])) {
        self.inner.for_each(table.0, |entry| f(entry.vals));
    }
}
