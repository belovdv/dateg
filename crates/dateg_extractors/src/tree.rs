use std::any::TypeId;

use ahash::AHashMap;
use dateg::*;

pub use dateg_extractors_macro::index_tree;

pub trait Constructor: 'static {
    type Inputs: TokenTuple;
    type Output: TokenOpaqueMarker;
    type Enum: Eq;
    fn into_variant(_: Self::Inputs) -> Self::Enum;
    type Index;
    fn cost(_: Self::Inputs, index: &Self::Index, eg: &dateg::EGraph) -> Option<usize>;
}

pub trait CostFor<T: Token> {
    fn cost(&self, t: T, eg: &EGraph) -> Option<usize>;
}

pub trait IndexFor<Token: TokenOpaqueMarker> {
    type Enum: Copy + Eq;
    fn get_map(&self) -> &AHashMap<Token, (usize, Vec<Self::Enum>)>;
    fn get_map_mut(&mut self) -> &mut AHashMap<Token, (usize, Vec<Self::Enum>)>;

    fn value(&self, token: Token) -> Self::Enum {
        self.get_map()[&token].1[0]
    }

    fn get_full(&self, token: Token) -> Option<&(usize, Vec<Self::Enum>)> {
        self.get_map().get(&token)
    }

    /// If current cost eq cost - add new value, return false.
    ///
    /// If current cost gt cost - update cost, set the only value, return true.
    fn update(&mut self, token: Token, cost: usize, value: Self::Enum) -> bool {
        let entry = self.get_map_mut().entry(token);
        let entry = entry.or_insert_with(|| (usize::MAX, vec![]));
        if entry.0 > cost {
            *entry = (cost, vec![value]);
            return true;
        }
        if entry.0 == cost && !entry.1.contains(&value) {
            entry.1.push(value);
        }
        false
    }
}

#[derive(Default)]
pub struct Extractor<Index>(AHashMap<TypeId, Box<dyn Fn(&mut Index, &EGraph) -> bool>>);

impl<Index: Default> Extractor<Index> {
    pub fn extract(self, eg: &EGraph) -> Index {
        let mut index = Default::default();
        let mut keep_going = true;
        while keep_going {
            keep_going = false;
            for cst in self.0.values() {
                keep_going |= (cst)(&mut index, eg);
            }
        }
        index
    }

    pub fn set_constructor<C: Constructor<Index = Index>, S>(&mut self, table: Table<S>)
    where
        Index: IndexFor<C::Output, Enum = C::Enum>,
        S: Schema<Inputs = C::Inputs, Output = C::Output, AllowAdd = True>,
    {
        self.0.insert(
            TypeId::of::<C>(),
            Box::new(move |index, eg| {
                let mut updated = false;
                eg.for_each_row(table, |inputs, output| {
                    if let Some(cost) = C::cost(inputs, index, eg) {
                        updated |= index.update(output, cost, C::into_variant(inputs));
                    };
                });
                updated
            }),
        );
    }
}

impl<T: BaseValue, I> CostFor<TokenPrimitive<T>> for I {
    fn cost(&self, _: TokenPrimitive<T>, _: &EGraph) -> Option<usize> {
        Some(0)
    }
}
impl<T: Send + Sync + 'static, Index: IndexFor<TokenOpaque<T>>> CostFor<TokenOpaque<T>> for Index {
    fn cost(&self, t: TokenOpaque<T>, _: &EGraph) -> Option<usize> {
        self.get_map().get(&t).map(|(cost, _)| *cost)
    }
}
impl<T: ContainerValueExt, Index: IndexFor<T::Element>> CostFor<TokenContainer<T>> for Index {
    fn cost(&self, t: TokenContainer<T>, eg: &EGraph) -> Option<usize> {
        let map = self.get_map();
        ContainerValueExt::iter(&*t.get(eg))
            .map(|t| map.get(&t).map(|(cost, _)| *cost))
            .fold(Some(0), |was, new| Some(was? + new?))
    }
}
