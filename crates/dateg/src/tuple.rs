use std::any::TypeId;

use egglog_bridge::{ColumnTy, QueryEntry};
use egglog_core_relations::Value;

use crate::{Entry, RuleBuilder, Token};

pub trait Tuple {
    const LEN: usize;
    type Array<T>;
    type TypeMapped<M: TypeMapper>;

    fn type_ids() -> impl Iterator<Item = TypeId>;
}

pub trait TypeMapper {
    type Output<T>;
}

pub trait TokenTuple: Tuple + Copy + 'static {
    fn egglog(eg: &egglog_bridge::EGraph) -> Vec<ColumnTy>;
    fn into_values(self) -> Vec<Value>;
    fn opaque_into_values(self) -> impl Iterator<Item = Value>;
    fn from_values(values: &[Value]) -> Self;
}
pub trait EntryTuple: Tuple {
    fn into_entries(self, rb: &mut RuleBuilder) -> Vec<QueryEntry>;
}

macro_rules! count {
    () => { 0 };
    ($_head:ident $($tail:ident)*) => { 1 + count!($($tail)*) };
}
macro_rules! impl_tt {
    () => {};
    ($head:ident $($tail:ident)*) => {
        impl_tt!(@ $head $($tail)*);
        impl_tt!($($tail)*);
    };
    (@ $($T:ident)*) => {
        impl<$($T: 'static),*> Tuple for ($($T,)*) {
            const LEN: usize = count!($($T)*);
            type Array<T> = [T; count!($($T)*)];
            type TypeMapped<Ma: TypeMapper> = ($(Ma::Output<$T>,)*);

            fn type_ids() -> impl Iterator<Item = TypeId> {
                [$(TypeId::of::<$T>()),*].into_iter()
            }
        }
        impl<$($T: Token),*> TokenTuple for ($($T,)*) {
            fn egglog(eg: &egglog_bridge::EGraph) -> Vec<ColumnTy> {
                vec![$($T::egglog(eg)),*]
            }
            fn into_values(self) -> Vec<Value> {
                #[allow(non_snake_case)]
                let ($($T,)*) = self;
                vec![$($T.into_value()),*]
            }
            fn opaque_into_values(self) -> impl Iterator<Item = Value> {
                #[allow(non_snake_case)]
                let ($($T,)*) = self;
                [$($T.opaque_into_value()),*].into_iter().flatten()
            }
            fn from_values(values: &[Value]) -> Self {
                let mut iter = values.iter();
                let r = ( $($T::from_value(*iter.next().unwrap()),)* );
                assert!(iter.next().is_none());
                r
            }
        }
        impl<$($T: Token),*> EntryTuple for ($(Entry<$T>,)*) { paste::paste! {
            fn into_entries(self, rb: &mut RuleBuilder) -> Vec<QueryEntry> {
                let ($([< $T:snake >],)*) = self;
                vec![ $( [< $T:snake >].into_entry(rb) ),* ]
            }
        } }

        paste::paste! {
            impl<$($T,)* $([< $T U >]: Into<$T>),*>
            Into<($($T,)*)>
            for IntoTuple<($([< $T U >],)*)>
            {
                fn into(self) -> ($($T,)*) {
                    #[allow(non_snake_case)]
                    let ($([< $T U _var >],)*) = self.0;
                    ($(Into::<$T>::into([< $T U _var >]),)*)
                }
            }
        }
    };
}
impl_tt!(A B C D E F G H I J K L M N O P);
pub struct IntoTuple<T>(pub T);

impl Tuple for () {
    const LEN: usize = 0;
    type Array<T> = [T; 0];
    type TypeMapped<M: TypeMapper> = ();

    fn type_ids() -> impl Iterator<Item = TypeId> {
        [].into_iter()
    }
}
impl TokenTuple for () {
    fn egglog(_: &egglog_bridge::EGraph) -> Vec<ColumnTy> {
        vec![]
    }
    fn into_values(self) -> Vec<Value> {
        vec![]
    }
    fn opaque_into_values(self) -> impl Iterator<Item = Value> {
        [].into_iter()
    }
    fn from_values(_: &[Value]) -> Self {
        ()
    }
}
impl EntryTuple for () {
    fn into_entries(self, _: &mut RuleBuilder) -> Vec<QueryEntry> {
        vec![]
    }
}
impl Into<()> for IntoTuple<()> {
    fn into(self) -> () {
        ()
    }
}
