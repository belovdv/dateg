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
    fn add_table<S: Schema>(
        &mut self,
        name: impl ToString,
        default: DefaultVal,
        merge: MergeFn,
    ) -> Table<S> {
        let name = name.to_string();
        let tid = TypeId::of::<S>();
        if let Some((tid_, id)) = self.tables.get(&name).copied() {
            assert_eq!(tid_, tid);
            return Table::from_egglog(id);
        }
        let id = self.inner.add_table(FunctionConfig {
            schema: S::egglog(&self.inner),
            default,
            merge,
            name: name.to_string(),
            can_subsume: false,
        });
        self.tables.insert(name.to_string(), (tid, id));
        Table::from_egglog(id)
    }
    pub fn get_table<S: Schema>(&self, name: &str) -> Table<S> {
        let (tid, id) = self.tables[name];
        assert_eq!(tid, TypeId::of::<S>(), "{name}");
        Table::from_egglog(id)
    }

    pub fn add_table_constructor<Inputs: TokenTuple, Output: TokenOpaqueMarker>(
        &mut self,
        name: impl ToString,
    ) -> Table<(Inputs, Output, True)> {
        self.add_table(name, DefaultVal::FreshId, MergeFn::UnionId)
    }
    pub fn add_table_function<Inputs: TokenTuple, Output: Token>(
        &mut self,
        name: impl ToString,
    ) -> Table<(Inputs, Output, False)> {
        self.add_table(name, DefaultVal::Fail, MergeFn::AssertEq)
    }
    pub fn add_table_relation<Inputs: TokenTuple>(
        &mut self,
        name: impl ToString,
    ) -> Table<(Inputs, TokenPrimitive<()>, False)> {
        let v0 = DefaultVal::Const(Value::new_const(0));
        self.add_table(name, v0, MergeFn::AssertEq)
    }
    /// Merge: old, new -> merged
    pub fn add_table_function_with_merge<Inputs: TokenTuple, Output: Token>(
        &mut self,
        name: impl ToString,
        merge: Function<((Output, Output), Output, True)>,
    ) -> Table<(Inputs, Output, False)> {
        self.add_table(
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

    pub fn _dbg_definitions(&self, value: Value) {
        for (name, fid) in self.tables.iter() {
            self.inner.for_each(fid.1, |entry| {
                if entry.vals.last() == Some(&value) {
                    eprintln!("values is last in {name}");
                }
            });
        }
    }
}
