use std::marker::PhantomData;

use egglog_bridge::{QueryEntry, RuleId};

use crate::*;

pub enum Entry<T> {
    Const(T),
    Var(Var<T>),
}
impl<T: Token> Clone for Entry<T> {
    fn clone(&self) -> Self {
        match self {
            Self::Const(arg0) => Self::Const(arg0.clone()),
            Self::Var(arg0) => Self::Var(arg0.clone()),
        }
    }
}
impl<T: Token> Copy for Entry<T> {}
pub struct Var<T>(usize, PhantomData<T>);
impl<T> Clone for Var<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone(), self.1.clone())
    }
}
impl<T> Copy for Var<T> {}

impl<T: Token> Entry<T> {
    pub fn into_entry(self, rb: &mut RuleBuilder) -> QueryEntry {
        match self {
            Self::Const(c) => QueryEntry::Const {
                val: c.into_value(),
                ty: T::egglog(rb.inner.egraph()),
            },
            Self::Var(var) => rb.vars[var.0].clone(),
        }
    }
}

#[allow(type_alias_bounds)]
type InputEntries<S: Schema> = <S::Inputs as Tuple>::TypeMapped<MapToEntry>;
pub struct MapToEntry;
impl TypeMapper for MapToEntry {
    type Output<T> = Entry<T>;
}

impl EGraph {
    pub fn rule_builder(&mut self) -> RuleBuilder<'_> {
        RuleBuilder {
            inner: self.inner.new_rule("", true),
            vars: vec![],
        }
    }
}

pub struct RuleBuilder<'a> {
    inner: egglog_bridge::RuleBuilder<'a>,
    vars: Vec<QueryEntry>,
}

impl<'a> RuleBuilder<'a> {
    fn _add_var<T: Token>(&mut self, var: QueryEntry) -> Var<T> {
        let id = self.vars.len();
        self.vars.push(var);
        Var(id, PhantomData)
    }
    pub fn var<T: Token>(&mut self) -> Var<T> {
        let var = self.inner.new_var(T::egglog(self.inner.egraph()));
        self._add_var(QueryEntry::Var(var))
    }
    pub fn var_named<T: Token>(&mut self, name: &str) -> Var<T> {
        let var = self
            .inner
            .new_var_named(T::egglog(self.inner.egraph()), name);
        self._add_var(var)
    }

    /// LHS: query table
    pub fn query<S: Schema>(
        &mut self,
        table: Table<S>,
        inputs: InputEntries<S>,
        output: Entry<S::Output>,
    ) where
        InputEntries<S>: EntryTuple,
    {
        let mut entries = inputs.into_entries(self);
        entries.push(output.into_entry(self));
        self.inner
            .query_table(table.egglog(), &entries, None)
            .unwrap();
    }
    /// RHS: use constructor
    pub fn add<S: Schema<DefaultVal = DefaultValFreshId>>(
        &mut self,
        table: Table<S>,
        inputs: InputEntries<S>,
    ) -> Var<S::Output>
    where
        InputEntries<S>: EntryTuple,
    {
        let entries = inputs.into_entries(self);
        let var = self
            .inner
            .lookup(table.egglog(), &entries, || "".to_string());
        self._add_var(QueryEntry::Var(var))
    }
    /// RHS: set value
    pub fn set<S: Schema>(
        &mut self,
        table: Table<S>,
        inputs: InputEntries<S>,
        output: Entry<S::Output>,
    ) where
        InputEntries<S>: EntryTuple,
    {
        let mut entries = inputs.into_entries(self);
        entries.push(output.into_entry(self));
        self.inner.set(table.egglog(), &entries);
    }
    /// RHS: Union two ids
    pub fn union<T: TokenValueOpaqueMarker>(&mut self, a: Entry<T>, b: Entry<T>) {
        let a = a.into_entry(self);
        let b = b.into_entry(self);
        self.inner.union(a, b);
    }

    pub fn build(self) -> RuleId {
        self.inner.build()
    }
}
