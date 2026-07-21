use std::{hash::Hash, marker::PhantomData};

use egglog_bridge::ColumnTy;
use egglog_core_relations::{BaseValue, Value};
use egglog_numeric_id::NumericId;

pub trait EGraphValue {
    type Token: Token;
}
impl<T: BaseValue> EGraphValue for T {
    type Token = TokenPrimitive<Self>;
}

pub trait Token: Copy + Eq + Hash + Send + Sync + 'static {
    fn from_egglog(egglog: Value) -> Self;
    fn into_egglog(self) -> Value;
    fn column_ty(eg: &egglog_bridge::EGraph) -> ColumnTy;
    fn as_opaque(self) -> Option<Value>;
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct TokenPrimitive<T: BaseValue>(Value, PhantomData<T>);
impl<T: BaseValue> Copy for TokenPrimitive<T> {}

// TODO: specify opaque value kept in the container
// #[derive(Clone, PartialEq, Eq, Hash)]
// pub struct TokenContainer<T: ContainerValue>(Value, PhantomData<T>);
// impl<T: ContainerValue> Copy for TokenContainer<T> {}

pub struct TokenOpaque<T: Send + Sync + 'static>(Value, PhantomData<T>);

#[rustfmt::skip]
impl<T: BaseValue> Token for TokenPrimitive<T> {
    fn from_egglog(egglog: Value) -> Self               { Self(egglog, PhantomData) }
    fn into_egglog(self) -> Value                       { self.0 }
    fn as_opaque(self) -> Option<Value>                 { None }
    fn column_ty(eg: &egglog_bridge::EGraph) -> ColumnTy {
        ColumnTy::Base(eg.base_values().get_ty::<T>())
    }
}

// #[rustfmt::skip]
// impl<T: ContainerValue> Token for TokenContainer<T> {
//     fn from_egglog(egglog: Value) -> Self               { Self(egglog, PhantomData) }
//     fn into_egglog(self) -> Value                       { self.0 }
//     fn as_opaque(self) -> Option<Value>                 { None }
//     fn column_ty(_: &egglog_bridge::EGraph) -> ColumnTy { ColumnTy::Id }
// }

#[rustfmt::skip]
impl<T: Send + Sync+'static> Token for TokenOpaque<T> {
    fn from_egglog(egglog: Value) -> Self               { Self(egglog, PhantomData) }
    fn into_egglog(self) -> Value                       { self.0 }
    fn as_opaque(self) -> Option<Value>                 { Some(self.0) }
    fn column_ty(_: &egglog_bridge::EGraph) -> ColumnTy { ColumnTy::Id }
}

impl<T: BaseValue> TokenPrimitive<T> {
    /// Note: this can call [`T::clone`] internally
    pub fn get(&self, eg: &crate::EGraph) -> T {
        eg.inner.base_values().unwrap::<T>(self.0)
    }
}

// impl<T: ContainerValue> TokenContainer<T> {
//     pub fn canon(&self, eg: &crate::EGraph) -> Self {
//         Self::from_egglog(eg.inner.get_canon_repr(self.0, ColumnTy::Id))
//     }
// }

impl<T: Send + Sync + 'static> TokenOpaque<T> {
    pub fn canon(&self, eg: &crate::EGraph) -> Self {
        Self::from_egglog(eg.inner.get_canon_repr(self.0, ColumnTy::Id))
    }
}

pub trait TokenOpaqueMarker: Token {
    type Inner: Send + Sync + 'static;
    fn as_token_opaque(self) -> TokenOpaque<Self::Inner>;
}
impl<T: Send + Sync + 'static> TokenOpaqueMarker for TokenOpaque<T> {
    type Inner = T;
    fn as_token_opaque(self) -> TokenOpaque<Self::Inner> {
        self
    }
}
pub trait TokenPrimitiveMarker: Token {
    type Inner: BaseValue;
}
impl<T: BaseValue> TokenPrimitiveMarker for TokenPrimitive<T> {
    type Inner = T;
}

impl<T: Send + Sync + 'static> Clone for TokenOpaque<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone(), self.1.clone())
    }
}
impl<T: Send + Sync + 'static> Copy for TokenOpaque<T> {}
impl<T: Send + Sync + 'static> PartialEq for TokenOpaque<T> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0 && self.1 == other.1
    }
}
impl<T: Send + Sync + 'static> Eq for TokenOpaque<T> {}
impl<T: Send + Sync + 'static> Hash for TokenOpaque<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state);
        self.1.hash(state);
    }
}

impl<T: BaseValue> std::fmt::Debug for TokenPrimitive<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!(
            "{}({})",
            short_type_name::<T>(),
            self.0.index()
        ))
    }
}
impl<T: Send + Sync + 'static> std::fmt::Debug for TokenOpaque<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!(
            "{}({})",
            short_type_name::<T>(),
            self.0.index()
        ))
    }
}
fn short_type_name<T>() -> &'static str {
    std::any::type_name::<T>().split("::").last().unwrap()
}
