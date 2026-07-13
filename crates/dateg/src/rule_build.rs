use std::marker::PhantomData;

use egglog_bridge::{QueryEntry, RuleId};

use crate::*;

impl EGraph {
    pub fn rule_builder(&mut self, name: Option<&str>) -> RuleBuilder<'_> {
        RuleBuilder {
            inner: self.inner.new_rule(name.unwrap_or(""), true),
            vars: vec![],
        }
    }
}

pub struct RuleBuilder<'a> {
    inner: egglog_bridge::RuleBuilder<'a>,
    vars: Vec<QueryEntry>,
}

#[derive(Clone, Copy)]
pub enum Entry<T: Token> {
    Const(T),
    Var(Var<T>),
}
#[derive(Clone, Copy)]
pub struct Var<T: Token>(usize, PhantomData<T>);

impl<T: Token> Entry<T> {
    pub fn into_entry(self, rb: &mut RuleBuilder) -> QueryEntry {
        match self {
            Self::Const(c) => QueryEntry::Const {
                val: c.into_egglog(),
                ty: T::column_ty(rb.inner.egraph()),
            },
            Self::Var(var) => rb.vars[var.0].clone(),
        }
    }
}

impl<'a> RuleBuilder<'a> {
    fn _add_var<T: Token>(&mut self, var: QueryEntry) -> Var<T> {
        let id = self.vars.len();
        self.vars.push(var);
        Var(id, PhantomData)
    }
    pub fn var<T: Token>(&mut self) -> Var<T> {
        let var = self.inner.new_var(T::column_ty(self.inner.egraph()));
        self._add_var(QueryEntry::Var(var))
    }
    pub fn var_named<T: Token>(&mut self, name: &str) -> Var<T> {
        let column_ty = T::column_ty(self.inner.egraph());
        let var = self.inner.new_var_named(column_ty, name);
        self._add_var(var)
    }

    /// LHS: query table
    pub fn query<S: Schema>(
        &mut self,
        q: impl LhsQuery<S>,
        inputs: <S::Inputs as TokenTuple>::Entries,
        output: Entry<S::Output>,
    ) {
        let mut entries = inputs.into_entries(self);
        entries.push(output.into_entry(self));
        q.query(&entries, self);
    }
    /// RHS: use constructor
    pub fn add<S: Schema<AllowAdd = True>>(
        &mut self,
        table: impl RhsAdd<S>,
        inputs: <S::Inputs as TokenTuple>::Entries,
    ) -> Var<S::Output> {
        let entries = inputs.into_entries(self);
        let var = table.add(&entries, self);
        self._add_var(var)
    }
    /// RHS: set value
    pub fn set<S: Schema>(
        &mut self,
        table: Table<S>,
        inputs: <S::Inputs as TokenTuple>::Entries,
        output: Entry<S::Output>,
    ) {
        let mut entries = inputs.into_entries(self);
        entries.push(output.into_entry(self));
        self.inner.set(table.into_egglog(), &entries);
    }
    /// RHS: Union two ids
    pub fn union<T: TokenOpaqueMarker>(&mut self, a: Entry<T>, b: Entry<T>) {
        let a = a.into_entry(self);
        let b = b.into_entry(self);
        self.inner.union(a, b);
    }

    pub fn build(self) -> RuleId {
        self.inner.build()
    }
}

pub trait LhsQuery<S: Schema> {
    fn query(&self, entries: &[QueryEntry], rb: &mut RuleBuilder);
}
impl<S: Schema> LhsQuery<S> for Table<S> {
    fn query(&self, entries: &[QueryEntry], rb: &mut RuleBuilder) {
        rb.inner
            .query_table(self.into_egglog(), entries, None)
            .unwrap();
    }
}
impl<S: Schema> LhsQuery<S> for Function<S> {
    fn query(&self, entries: &[QueryEntry], rb: &mut RuleBuilder) {
        let column_ty = S::Output::column_ty(rb.inner.egraph());
        rb.inner
            .query_prim(self.into_egglog(), entries, column_ty)
            .unwrap();
    }
}

pub trait RhsAdd<S: Schema> {
    fn add(&self, entries: &[QueryEntry], rb: &mut RuleBuilder) -> QueryEntry;
}
impl<S: Schema> RhsAdd<S> for Table<S> {
    fn add(&self, entries: &[QueryEntry], rb: &mut RuleBuilder) -> QueryEntry {
        let tid = self.into_egglog();
        QueryEntry::Var(rb.inner.lookup(tid, &entries, || "".to_string()))
    }
}
impl<S: Schema> RhsAdd<S> for Function<S> {
    fn add(&self, entries: &[QueryEntry], rb: &mut RuleBuilder) -> QueryEntry {
        let fid = self.into_egglog();
        let column_ty = S::Output::column_ty(rb.inner.egraph());
        QueryEntry::Var(
            rb.inner
                .call_external_func(fid, entries, column_ty, || "".to_string()),
        )
    }
}
