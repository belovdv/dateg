/*
while keep_going
    for table in tables
        for inputs, output in table.rows
            cost = 1 + cost(inputs)
            index.update(output, cost, $Sort::$Table(inputs))

table+inputs -> cost sum
    for token in inputs
        cost += index.get_$token(token).0
alternative
    cost_index.get((FunctionId, inputs.into_values))

sort: update entry
    entry = index.$sort.entry(output).or_insert_with(usize::MAX, vec![])
    if entry.0 == cost
        entry.1.push(impl)
    if entry.0 > cost
        entry = cost, vec![impl]

*/

use dateg::Token;

#[macro_export]
macro_rules! tree_basic {
    ($Index:ident::$extract:ident $(
        (datatype $Token:ident -> $Sort:ident $(
            $Constructor:ident ( $($Args:ident)* ) $( :cost $cost:literal )?
        )+)
    )*) => { paste::paste! {
        #[derive(Default)]
        pub struct $Index {$(
            pub [< $Sort:snake >]: $crate::AHashMap<$Token, (usize, Vec<$Sort>)>,
        )*}
        $(#[derive(Clone, Copy, PartialEq, Eq)]
        pub enum $Sort {$(
            $Constructor(($($Args,)*)),
        )*})*
        $(impl $crate::tree_basic::IndexGet<$Token> for $Index {
            type Impl = $Sort;
            fn get(&self, t: $Token) -> Option<&(usize, Vec<Self::Impl>)> {
                self.[< $Sort:snake >].get(&t)
            }
            fn update(&mut self, t: $Token, cost: usize, imp: Self::Impl) -> bool {
                let entry = self.[< $Sort:snake >].entry(t).or_insert_with(|| (usize::MAX, vec![]));
                if entry.0 == cost {
                    if !entry.1.contains(&imp) {
                        entry.1.push(imp);
                    }
                } else if entry.0 > cost {
                    entry.0 = cost;
                    entry.1 = vec![imp];
                    return true;
                }
                false
            }
        })*

        impl $Index { pub fn $extract<
            $($( [< $Constructor Schema >]: dateg::Schema<Inputs = ($($Args,)*), Output = $Token>, )*)*
        >(eg: &dateg::EGraph $(,
            ($([< $Constructor:snake >]),*):
            ($(dateg::Table<[< $Constructor Schema >]>),*)
        )*) -> $Index {
            use $crate::tree_basic::IndexGet;
            $crate::tuple_scanner!(Cost<'a>(Option<usize>, &'a $Index);
                $( fn (t: $Token, c) -> () {
                    if let Some(cost) = c.0 {
                        if let Some((delta, _)) = c.1.get(t) {
                            c.0 = Some(cost + delta);
                        } else {
                            c.0 = None;
                        }
                    }
                } )*
                fn <T: dateg::BaseValue>(_: dateg::TokenValuePrimitive<T>, _) -> () {}
            );

            let mut r = $Index::default();
            let mut keep_going = true;
            while keep_going {
                keep_going = false;
                $($( eg.for_each_row([< $Constructor:snake >], |inputs, output| {
                    let _cost = 1;
                    $( let _cost = $cost; )?;
                    let mut scanner = Cost(Some(_cost), &r);
                    scanner.scan(inputs);
                    let Some(cost) = scanner.0 else {
                        return;
                    };
                    keep_going |= r.update(output, cost, $Sort::$Constructor(inputs));
                }); )*)*
            }
            r
        }}
    }}
}

pub trait IndexGet<T: Token> {
    type Impl: Copy;
    fn get(&self, t: T) -> Option<&(usize, Vec<Self::Impl>)>;
    fn update(&mut self, t: T, cost: usize, imp: Self::Impl) -> bool;
    fn get_first(&self, t: T) -> Self::Impl {
        self.get(t).unwrap().1[0]
    }
}
