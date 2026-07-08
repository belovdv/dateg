use std::marker::PhantomData;

use egglog_bridge::FunctionId;
use egglog_core_relations::Value;

use crate::*;

pub struct Table<S: Schema>(FunctionId, PhantomData<S>);
impl<S: Schema> Clone for Table<S> {
    fn clone(&self) -> Self {
        Self(self.0.clone(), self.1.clone())
    }
}
impl<S: Schema> Copy for Table<S> {}

impl<S: Schema> Table<S> {
    pub fn egglog(&self) -> FunctionId {
        self.0
    }
}

impl EGraph {
    /// If name already exists - returns handle to corresponding table.
    ///
    /// Panics if schema of that table doesn't match requested one.
    pub fn add_table<S: Schema>(&mut self, name: impl ToString) -> Table<S> {
        let name = name.to_string();
        let id = match self.tables.entry(name.clone()) {
            std::collections::hash_map::Entry::Occupied(entry) => {
                assert_eq!(entry.get().0, TypeId::of::<S>());
                entry.get().1
            }
            std::collections::hash_map::Entry::Vacant(entry) => {
                let id = self.inner.add_table(S::egglog(&self.inner, name));
                entry.insert((TypeId::of::<S>(), id));
                id
            }
        };
        Table(id, PhantomData)
    }
    pub fn add_table_constructor<In: TokenTuple, Out: TokenValueOpaqueMarker>(
        &mut self,
        name: impl ToString,
    ) -> Table<TableConstructorSchema<In, Out>> {
        self.add_table(name)
    }
    pub fn add_table_function<In: TokenTuple, Out: Token>(
        &mut self,
        name: impl ToString,
    ) -> Table<TableFunctionSchema<In, Out>> {
        self.add_table(name)
    }
    pub fn add_table_relation<Body: TokenTuple>(
        &mut self,
        name: impl ToString,
    ) -> Table<TableRelationSchema<Body>> {
        self.add_table(name)
    }

    pub fn for_each_row<S: Schema>(
        &self,
        table: Table<S>,
        mut f: impl FnMut(S::Inputs, S::Output),
    ) {
        self.inner.for_each(table.0, |entry| {
            let (output, inputs) = entry.vals.split_last().unwrap();
            let inputs = S::Inputs::from_values(inputs);
            let output = S::Output::from_value(*output);
            f(inputs, output);
        });
    }

    pub fn loader<S: Schema>(&mut self, table: Table<S>) -> Loader<'_, S> {
        Loader {
            eg: self,
            _p: PhantomData,
            id: table.0,
            values: vec![],
        }
    }

    pub fn row_set<S: Schema>(&mut self, table: Table<S>, key: S::Inputs, value: S::Output) {
        let mut loader = self.loader(table);
        loader.set(key, value);
        loader.flush();
    }
    pub fn row_get<TVO, S>(&self, table: Table<S>, key: S::Inputs) -> Option<S::Output>
    where
        TVO: Send + Sync + 'static,
        S: Schema<Output = TokenValueOpaque<TVO>>,
    {
        self.inner
            .lookup_id(table.0, &key.into_values())
            .map(TokenValueOpaque::from_value)
    }
    pub fn row_add<S: Schema<DefaultVal = DefaultValFreshId>>(
        &mut self,
        table: Table<S>,
        key: S::Inputs,
    ) -> S::Output {
        let mut loader = self.loader(table);
        let r = loader.add(key);
        loader.flush();
        r
    }
}

pub struct Loader<'a, S: Schema> {
    eg: &'a mut EGraph,
    _p: PhantomData<S>,
    id: FunctionId,
    values: Vec<(FunctionId, Vec<Value>)>,
}

impl<'a, S: Schema> Loader<'a, S> {
    // pub fn set_table<S2: Schema>(self, table: Table<S2>) -> Loader<'a, S2> {
    //     let Self { eg, values, .. } = self;
    //     let _p = PhantomData;
    //     let id = table.0;
    //     Loader { eg, _p, id, values }
    // }

    fn add_values(&mut self, values: Vec<Value>) {
        self.values.push((self.id, values));
    }

    pub fn set(&mut self, key: S::Inputs, value: S::Output) {
        let mut values = key.into_values();
        values.push(value.into_value());
        self.add_values(values);
    }

    pub fn flush(&mut self) {
        if !self.values.is_empty() {
            self.eg.inner.add_values(std::mem::take(&mut self.values));
        }
    }
}
impl<'a, S: Schema<DefaultVal = DefaultValFreshId>> Loader<'a, S> {
    pub fn add(&mut self, key: S::Inputs) -> S::Output {
        let mut values = key.into_values();
        let r = self.eg.inner.fresh_id();
        values.push(r);
        self.add_values(values);
        S::Output::from_value(r)
    }
}
impl<'a, S: Schema> Drop for Loader<'a, S> {
    fn drop(&mut self) {
        assert!(self.values.is_empty())
    }
}
