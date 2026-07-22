use std::hash::Hash;

use egglog_bridge::{ColumnTy, QueryEntry};
use egglog_core_relations::{ExecutionState, Value};

use crate::*;

pub trait Schema: Copy + Eq + Hash + Send + Sync + 'static {
    type Inputs: TokenTuple;
    type Output: Token;
    type AllowAdd: Bool;

    fn egglog(eg: &egglog_bridge::EGraph) -> Vec<ColumnTy> {
        let mut r = Self::Inputs::column_ty(eg);
        r.push(Self::Output::column_ty(eg));
        r
    }
}

impl<Inputs: TokenTuple, Output: Token, AllowAdd: Bool> Schema for (Inputs, Output, AllowAdd) {
    type Inputs = Inputs;
    type Output = Output;
    type AllowAdd = AllowAdd;
}

pub trait TokenTuple: Copy + Eq + Hash + Send + Sync + 'static {
    fn column_ty(eg: &egglog_bridge::EGraph) -> Vec<ColumnTy>;
    fn from_egglog(values: &[Value]) -> Self;
    fn into_egglog(self) -> impl Iterator<Item = Value>;
    fn into_egglog_vec(self) -> Vec<Value> {
        self.into_egglog().collect()
    }

    type Entries: IntoEntries;

    fn type_ids() -> impl Iterator<Item = TypeId>;
    fn into_non_primitive(self) -> impl Iterator<Item = Value>;
}
pub trait IntoEntries {
    fn into_entries(self, rb: &mut RuleBuilder) -> Vec<QueryEntry>;
}
pub trait TokenTuplePartialResolve: TokenTuple {
    type Resolved: Send + Sync + 'static;
    fn partial_resolve(self, es: &ExecutionState) -> Self::Resolved;
}
pub trait TokenPartialResolve: Token {
    type Resolved: Send + Sync + 'static;
    fn partial_resolve(self, es: &ExecutionState) -> Self::Resolved;
}
impl<T: BaseValue> TokenPartialResolve for TokenPrimitive<T> {
    type Resolved = T;
    fn partial_resolve(self, es: &ExecutionState) -> Self::Resolved {
        es.base_values().unwrap(self.into_egglog())
    }
}
impl<T: Send + Sync + 'static> TokenPartialResolve for TokenOpaque<T> {
    type Resolved = Self;
    fn partial_resolve(self, _: &ExecutionState) -> Self::Resolved {
        self
    }
}
impl<T: ContainerValueExt> TokenPartialResolve for TokenContainer<T> {
    type Resolved = T;
    fn partial_resolve(self, es: &ExecutionState) -> Self::Resolved {
        // Is it possible to avoid cloning?
        es.container_values()
            .get_val::<T>(self.into_egglog())
            .unwrap()
            .clone()
    }
}

macro_rules! impl_tt {
    () => {};
    ($head:ident $($tail:ident)*) => {
        impl_tt!(@ $($tail)*);
        impl_tt!($($tail)*);
    };
    (@ $($T:ident)*) => {
        #[allow(unused)]
        impl<$($T: Token),*> TokenTuple for ($($T,)*) {
            fn column_ty(eg: &egglog_bridge::EGraph) -> Vec<ColumnTy> {
                vec![$($T::column_ty(eg)),*]
            }
            fn from_egglog(values: &[Value]) -> Self {
                let mut iter = values.iter();
                let r = ( $($T::from_egglog(*iter.next().unwrap()),)* );
                assert!(iter.next().is_none());
                r
            }
            fn into_egglog(self) -> impl Iterator<Item = Value> {
                #[allow(non_snake_case)]
                let ($($T,)*) = self;
                [$($T.into_egglog()),*].into_iter()
            }
            type Entries = ($(Entry<$T>,)*);
            fn type_ids() -> impl Iterator<Item = TypeId> {
                [$(TypeId::of::<$T>()),*].into_iter()
            }
            fn into_non_primitive(self) -> impl Iterator<Item = Value> {
                #[allow(non_snake_case)]
                let ($($T,)*) = self;
                <[Option<Value>; _]>::into_iter([$($T.as_non_primitive()),*]).flatten()
            }
        }
        #[allow(unused)]
        impl<$($T: Token),*> IntoEntries for ($(Entry<$T>,)*) {
            fn into_entries(self, rb: &mut RuleBuilder) -> Vec<QueryEntry> {
                #[allow(non_snake_case)]
                let ($($T,)*) = self;
                vec![$($T.into_entry(rb)),*]
            }
        }
        #[allow(unused)]
        impl<$($T: TokenPartialResolve),*> TokenTuplePartialResolve for ($($T,)*) {
            type Resolved = ($($T::Resolved,)*);
            fn partial_resolve(self, es: &ExecutionState) -> Self::Resolved {
                #[allow(non_snake_case)]
                let ($($T,)*) = self;
                ($($T.partial_resolve(es),)*)
            }
        }
    };
}
impl_tt!(A B C D E F G H I J K L);
