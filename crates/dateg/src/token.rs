use std::marker::PhantomData;

use egglog_bridge::ColumnTy;
use egglog_core_relations::{BaseValue, Value};

use crate::EGraph;

pub trait Token: Copy + Eq + 'static {
    fn from_value(value: Value) -> Self;
    fn into_value(self) -> Value;
    fn egglog_column_ty(eg: &EGraph) -> ColumnTy;
    fn canon(&self, eg: &EGraph) -> Self;
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TokenValuePrimitive<T: BaseValue> {
    id: Value,
    _p: PhantomData<T>,
}
impl<T: BaseValue> Copy for TokenValuePrimitive<T> {}

impl<T: BaseValue> Token for TokenValuePrimitive<T> {
    fn from_value(id: Value) -> Self {
        let _p = PhantomData;
        Self { id, _p }
    }
    fn into_value(self) -> Value {
        self.id
    }
    fn egglog_column_ty(eg: &EGraph) -> ColumnTy {
        ColumnTy::Base(eg.inner.base_values().get_ty::<T>())
    }
    fn canon(&self, _: &EGraph) -> Self {
        *self
    }
}
impl<T: BaseValue> TokenValuePrimitive<T> {
    /// Note: this can call [`T::clone`] internally
    pub fn get(&self, eg: &EGraph) -> T {
        eg.inner.base_values().unwrap::<T>(self.id)
    }
}

#[derive(Debug)]
pub struct TokenValueOpaque<T: 'static> {
    id: Value,
    _p: PhantomData<T>,
}
impl<T> Clone for TokenValueOpaque<T> {
    fn clone(&self) -> Self {
        Self {
            id: self.id.clone(),
            _p: self._p.clone(),
        }
    }
}
impl<T> Copy for TokenValueOpaque<T> {}
impl<T> PartialEq for TokenValueOpaque<T> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && self._p == other._p
    }
}
impl<T> Eq for TokenValueOpaque<T> {}
impl<T> std::hash::Hash for TokenValueOpaque<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
        self._p.hash(state);
    }
}

impl<T> Token for TokenValueOpaque<T> {
    fn from_value(id: Value) -> Self {
        let _p = PhantomData;
        Self { id, _p }
    }
    fn into_value(self) -> Value {
        self.id
    }
    fn egglog_column_ty(_: &EGraph) -> ColumnTy {
        ColumnTy::Id
    }
    fn canon(&self, eg: &EGraph) -> Self {
        Self::from_value(eg.inner.get_canon_repr(self.id, ColumnTy::Id))
    }
}

pub trait TokenValueOpaqueMarker: Token {}
impl<T> TokenValueOpaqueMarker for TokenValueOpaque<T> {}
