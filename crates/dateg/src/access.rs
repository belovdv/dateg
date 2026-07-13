use std::marker::PhantomData;

use crate::*;

impl EGraph {
    pub fn loader<S: Schema>(&mut self, table: Table<S>) -> Loader<'_, S> {
        Loader::new(self, table, vec![])
    }

    pub fn row_set<S: Schema>(&mut self, table: Table<S>, key: S::Inputs, value: S::Output) {
        let mut loader = self.loader(table);
        loader.set(key, value);
        loader.flush();
    }
    pub fn row_get<S: Schema>(&self, table: Table<S>, key: S::Inputs) -> Option<S::Output> {
        self.inner
            .lookup_id(table.into_egglog(), &key.into_egglog_vec())
            .map(S::Output::from_egglog)
    }
    pub fn row_add<S>(&mut self, table: Table<S>, key: S::Inputs) -> S::Output
    where
        S: Schema<AllowAdd = True>,
    {
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
    pub fn new(eg: &'a mut EGraph, table: Table<S>, values: Vec<(FunctionId, Vec<Value>)>) -> Self {
        let _p = PhantomData;
        let id = table.into_egglog();
        Self { eg, _p, id, values }
    }
    pub fn with_table<S2: Schema>(self, table: Table<S2>) -> Loader<'a, S2> {
        let Self { eg, values, .. } = self;
        Loader::new(eg, table, values)
    }

    fn add_values(&mut self, values: Vec<Value>) {
        self.values.push((self.id, values));
    }

    pub fn set(&mut self, key: S::Inputs, value: S::Output) {
        let mut values = key.into_egglog_vec();
        values.push(value.into_egglog());
        self.add_values(values);
    }

    pub fn flush(&mut self) {
        if !self.values.is_empty() {
            self.eg.inner.add_values(std::mem::take(&mut self.values));
        }
    }
}
impl<'a, S: Schema<AllowAdd = True>> Loader<'a, S> {
    pub fn add(&mut self, key: S::Inputs) -> S::Output {
        let mut values = key.into_egglog_vec();
        let r = self.eg.inner.fresh_id();
        values.push(r);
        self.add_values(values);
        S::Output::from_egglog(r)
    }
}
