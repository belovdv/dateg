use ahash::AHashMap;
use dateg::{EGraph, Schema, Table, Token, TokenTuple, Value};

use crate::index::{Constructor, IndexFor};

#[derive(Default)]
pub struct ExtractorTree<Index> {
    index: Index,
    costs: AHashMap<Value, usize>,
    constructors: Vec<Box<dyn Fn(&mut Index, &mut AHashMap<Value, usize>, &EGraph) -> bool>>,
}
impl<Index> ExtractorTree<Index> {
    pub fn extract(mut self, eg: &EGraph) -> Index {
        let mut keep_going = true;
        while keep_going {
            keep_going = false;
            for cst in self.constructors.iter() {
                keep_going |= (cst)(&mut self.index, &mut self.costs, eg);
            }
        }
        self.index
    }

    pub fn register_constructor<C: Constructor, S>(&mut self, table: Table<S>)
    where
        Index: IndexFor<C::Output, Sort = C::Sort>,
        S: Schema<Inputs = C::Inputs, Output = C::Output>,
    {
        self.constructors.push(Box::new(move |index, costs, eg| {
            let mut updated = false;
            C::for_each_row(eg, table, |inputs, output| {
                let mut cost = C::COST;
                for value in inputs.opaque_into_values() {
                    let Some(cost_opaque) = costs.get(&value) else {
                        return;
                    };
                    cost += *cost_opaque;
                }
                if index.update(output, cost, C::into_variant(inputs)) {
                    costs.insert(output.into_egglog(), cost);
                    updated = true;
                }
            });
            updated
        }));
    }
}
