//! Token - reference to value in database

use std::{hash::Hash, marker::PhantomData};

use egglog_bridge::ColumnTy;
use egglog_core_relations::{BaseValue, Value};

use crate::EGraph;

pub trait Token: Copy + Eq + Hash + Send + Sync + 'static {
    fn canon(&self, eg: &EGraph) -> Self;

    fn from_value(value: Value) -> Self;
    fn into_value(self) -> Value;
    fn egglog(eg: &egglog_bridge::EGraph) -> ColumnTy;
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct TokenValuePrimitive<T: BaseValue>(Value, PhantomData<T>);
impl<T: BaseValue> Copy for TokenValuePrimitive<T> {}

pub struct TokenValueOpaque<T>(Value, PhantomData<T>);
impl<T> Clone for TokenValueOpaque<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone(), self.1.clone())
    }
}
impl<T> Copy for TokenValueOpaque<T> {}
impl<T> PartialEq for TokenValueOpaque<T> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0 && self.1 == other.1
    }
}
impl<T> Eq for TokenValueOpaque<T> {}
impl<T> Hash for TokenValueOpaque<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state);
        self.1.hash(state);
    }
}

impl<T: BaseValue> Token for TokenValuePrimitive<T> {
    fn from_value(id: Value) -> Self {
        let _p = PhantomData;
        Self(id, _p)
    }
    fn into_value(self) -> Value {
        self.0
    }
    fn egglog(eg: &egglog_bridge::EGraph) -> ColumnTy {
        ColumnTy::Base(eg.base_values().get_ty::<T>())
    }
    fn canon(&self, _: &EGraph) -> Self {
        *self
    }
}
impl<T: BaseValue> TokenValuePrimitive<T> {
    /// Note: this can call [`T::clone`] internally
    pub fn get(&self, eg: &EGraph) -> T {
        eg.inner.base_values().unwrap::<T>(self.0)
    }
}

impl<T: Send + Sync + 'static> Token for TokenValueOpaque<T> {
    fn from_value(id: Value) -> Self {
        let _p = PhantomData;
        Self(id, _p)
    }
    fn into_value(self) -> Value {
        self.0
    }
    fn egglog(_: &egglog_bridge::EGraph) -> ColumnTy {
        ColumnTy::Id
    }
    fn canon(&self, eg: &EGraph) -> Self {
        Self::from_value(eg.inner.get_canon_repr(self.0, ColumnTy::Id))
    }
}

pub trait TokenValueOpaqueMarker: Token {}
impl<T: Send + Sync + 'static> TokenValueOpaqueMarker for TokenValueOpaque<T> {}
