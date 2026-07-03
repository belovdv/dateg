use egglog_bridge::ColumnTy;
use egglog_core_relations::Value;

use crate::Token;

pub trait TokenTuple {
    fn egglog(eg: &egglog_bridge::EGraph) -> Vec<ColumnTy>;
    fn into_values(self) -> Vec<Value>;
    fn from_values(values: &[Value]) -> Self;
}

macro_rules! impl_tt {
    () => {};
    ($head:ident $($tail:ident)*) => {
        impl_tt!(@ $head $($tail)*);
        impl_tt!($($tail)*);
    };
    (@ $($T:ident)*) => {
        impl<$($T: Token),*> TokenTuple for ($($T,)*) {
            fn egglog(eg: &egglog_bridge::EGraph) -> Vec<ColumnTy> {
                vec![$($T::egglog(eg)),*]
            }
            fn into_values(self) -> Vec<Value> {
                #[allow(non_snake_case)]
                let ($($T,)*) = self;
                vec![$($T.into_value()),*]
            }
            fn from_values(values: &[Value]) -> Self {
                let mut iter = values.iter();
                let r = ( $($T::from_value(*iter.next().unwrap()),)* );
                assert!(iter.next().is_none());
                r
            }
        }
    };
}
impl_tt!(A B C D E F G H I J K L M N O P);
