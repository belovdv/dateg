use ahash::AHashMap;
use dateg::{EGraph, Schema, Table, Token, TokenTuple};

pub trait Constructor: Copy + 'static {
    type Inputs: TokenTuple;
    fn into_inner(self) -> Self::Inputs;

    type Output: Token;
    /// Enum of variants
    type Sort;
    fn into_variant(_: Self::Inputs) -> Self::Sort;

    const COST: usize;

    fn for_each_row<S: Schema<Inputs = Self::Inputs, Output = Self::Output>>(
        eg: &EGraph,
        table: Table<S>,
        f: impl FnMut(Self::Inputs, Self::Output),
    );
}

pub trait IndexFor<Tok: Token> {
    /// Enum of variants
    type Sort: Copy + PartialEq;

    fn get_map(&self) -> &AHashMap<Tok, (usize, Vec<Self::Sort>)>;
    fn get_map_mut(&mut self) -> &mut AHashMap<Tok, (usize, Vec<Self::Sort>)>;

    fn get_full(&self, token: Tok) -> Option<&(usize, Vec<Self::Sort>)> {
        self.get_map().get(&token)
    }
    fn get_full_mut_or_insert(&mut self, token: Tok) -> &mut (usize, Vec<Self::Sort>) {
        self.get_map_mut()
            .entry(token)
            .or_insert_with(|| (usize::MAX, vec![]))
    }

    /// If current cost eq cost - add new value,
    /// if current cost gt cost - update cost, set the only value, return true.
    fn update(&mut self, token: Tok, cost: usize, value: Self::Sort) -> bool {
        let (cost_was, values) = self.get_full_mut_or_insert(token);
        if *cost_was > cost {
            *cost_was = cost;
            *values = vec![value];
            return true;
        }
        if *cost_was == cost {
            if !values.contains(&value) {
                values.push(value);
            }
        }
        false
    }

    fn value(&self, token: Tok) -> Self::Sort {
        self.get_full(token).unwrap().1[0]
    }
}

#[macro_export]
macro_rules! define_index {
    ($Index:ident $(
        (datatype $Token:ident -> $Sort:ident $(
            $Constructor:ident ( $($Args:ident)* ) $( :cost $cost:literal )?
        )+)
    )*) => { paste::paste! {
        #[derive(Default)]
        pub struct $Index {$(
            pub [< $Sort:snake >]: $crate::AHashMap<<$Token as dateg::EGraphValue>::Token, (usize, Vec<$Sort>)>,
        )*}
        impl $Index {
            pub fn extractor_tree_basic<
                $($( [< $Constructor Schema >]: dateg::Schema<Inputs = ($(<$Args as dateg::EGraphValue>::Token,)*), Output = <$Token as dateg::EGraphValue>::Token>, )*)*
            >(eg: &dateg::EGraph $(,
                ($([< $Constructor:snake >]),*):
                ($(dateg::Table<[< $Constructor Schema >]>),*)
            )* ) -> Self {
                let mut extractor = dateg_extractors::ExtractorTree::<Self>::default();
                $($(
                    extractor.register_constructor::<$Constructor, _>([< $Constructor:snake >]);
                )*)*
                extractor.extract(eg)
            }
            pub fn extractor_dag_basic<
                $($( [< $Constructor Schema >]: dateg::Schema<Inputs = ($(<$Args as dateg::EGraphValue>::Token,)*), Output = <$Token as dateg::EGraphValue>::Token>, )*)*
            >(eg: &dateg::EGraph $(,
                ($([< $Constructor:snake >]),*):
                ($(dateg::Table<[< $Constructor Schema >]>),*)
            )* ) -> dateg_extractors::ExtractorDAG::<Self> {
                let mut extractor = dateg_extractors::ExtractorDAG::<Self>::default();
                $($(
                    extractor.register_constructor::<$Constructor, _>([< $Constructor:snake >], 1$( + $cost - 1)?);
                )*)*
                extractor.init(eg);
                extractor
            }
        }
        $(
            #[derive(Clone, Copy, PartialEq, Eq)]
            pub enum $Sort {$(
                $Constructor(($(<$Args as dateg::EGraphValue>::Token,)*)),
            )*}
            impl $crate::IndexFor<<$Token as dateg::EGraphValue>::Token> for $Index {
                type Sort = $Sort;
                fn get_map(&self) -> &$crate::AHashMap<<$Token as dateg::EGraphValue>::Token, (usize, Vec<Self::Sort>)> {
                    &self.[< $Sort:snake >]
                }
                fn get_map_mut(&mut self) -> &mut $crate::AHashMap<<$Token as dateg::EGraphValue>::Token, (usize, Vec<Self::Sort>)> {
                    &mut self.[< $Sort:snake >]
                }
            }
            $(
                #[derive(Clone, Copy)]
                pub struct $Constructor(($(<$Args as dateg::EGraphValue>::Token,)*));
                impl $crate::Constructor for $Constructor {
                    type Inputs = ($(<$Args as dateg::EGraphValue>::Token,)*);
                    fn into_inner(self) -> ($(<$Args as dateg::EGraphValue>::Token,)*) {
                        self.0
                    }
                    type Output = <$Token as dateg::EGraphValue>::Token;
                    type Sort = $Sort;
                    fn into_variant(inputs: Self::Inputs) -> Self::Sort {
                        $Sort::$Constructor(inputs)
                    }
                    fn for_each_row<S: dateg::Schema<Inputs = Self::Inputs, Output = Self::Output>>(
                        eg: &dateg::EGraph, table: dateg::Table<S>, mut f: impl FnMut(Self::Inputs, Self::Output)
                    ) {
                        eg.for_each_row(table, |inputs, output| f(inputs, output));
                    }
                    const COST: usize = 1 $( + $cost - 1 )?;
                }
            )*
        )*
    }};
}
