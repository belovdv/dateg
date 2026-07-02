use std::marker::PhantomData;

use egglog_bridge::{ColumnTy, FunctionConfig, FunctionId};
use egglog_core_relations::Value;

use crate::{EGraph, Token, token::TokenValueOpaqueMarker};

pub struct TableToken<S: TableSchema> {
    id: FunctionId,
    _p: PhantomData<S>,
}
impl<S: TableSchema> Clone for TableToken<S> {
    fn clone(&self) -> Self {
        Self {
            id: self.id.clone(),
            _p: self._p.clone(),
        }
    }
}
impl<S: TableSchema> Copy for TableToken<S> {}

pub trait TableSchema {
    type InsertArguments;
    type InsertReturn;

    fn insert<'a>(
        loader: &mut Loader<'a, Self>,
        input: Self::InsertArguments,
    ) -> Self::InsertReturn;

    type Read: TokenTuple;

    fn egglog_function_config(eg: &EGraph, name: String) -> FunctionConfig;
}

pub trait TokenTuple {
    fn into_values(self) -> Vec<Value>;
    fn from_values(values: &[Value]) -> Self;
    fn egglog_column_ty(eg: &EGraph) -> Vec<ColumnTy>;
    type Push<T>;
}
macro_rules! impl_tt {
    () => {};
    ($head:ident $($tail:ident)*) => {
        impl_tt!(@ $head $($tail)*);
        impl_tt!($($tail)*);
    };
    (@ $($T:ident)*) => {
        impl<$($T: Token),*> TokenTuple for ($($T,)*) {
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
            fn egglog_column_ty(eg: &EGraph) -> Vec<ColumnTy> {
                vec![$($T::egglog_column_ty(eg)),*]
            }
            type Push<T> = ($($T,)* T,);
        }
    };
}
impl_tt!(A B C D E F G H I J);

pub struct TableSchemaConstructor<In: TokenTuple, Out: Token>(PhantomData<(In, Out)>);
pub struct TableSchemaFunction<In: TokenTuple, Out: Token>(PhantomData<(In, Out)>);
pub struct TableSchemaRelation<Body: TokenTuple>(PhantomData<Body>);

impl<In: TokenTuple, Out: Token> TableSchema for TableSchemaConstructor<In, Out>
where
    In::Push<Out>: TokenTuple,
{
    type InsertArguments = In;
    type InsertReturn = Out;
    fn insert<'a>(
        loader: &mut Loader<'a, Self>,
        input: Self::InsertArguments,
    ) -> Self::InsertReturn {
        let r = loader.eg.inner.fresh_id();
        let mut values = input.into_values();
        values.push(r);
        loader.add_values(values);
        Out::from_value(r)
    }
    type Read = In::Push<Out>;
    fn egglog_function_config(eg: &EGraph, name: String) -> FunctionConfig {
        let mut schema = In::egglog_column_ty(eg);
        schema.push(Out::egglog_column_ty(eg));
        FunctionConfig {
            schema,
            default: egglog_bridge::DefaultVal::FreshId,
            merge: egglog_bridge::MergeFn::UnionId,
            name,
            can_subsume: false,
        }
    }
}
impl<In: TokenTuple, Out: Token> TableSchema for TableSchemaFunction<In, Out>
where
    In::Push<Out>: TokenTuple,
{
    type InsertArguments = (In, Out);
    type InsertReturn = ();
    fn insert<'a>(
        loader: &mut Loader<'a, Self>,
        (input, output): Self::InsertArguments,
    ) -> Self::InsertReturn {
        let mut values = input.into_values();
        values.push(output.into_value());
        loader.add_values(values);
    }
    type Read = In::Push<Out>;
    fn egglog_function_config(eg: &EGraph, name: String) -> FunctionConfig {
        let mut schema = In::egglog_column_ty(eg);
        schema.push(Out::egglog_column_ty(eg));
        FunctionConfig {
            schema,
            default: egglog_bridge::DefaultVal::Fail,
            merge: egglog_bridge::MergeFn::AssertEq,
            name,
            can_subsume: false,
        }
    }
}
impl<Body: TokenTuple> TableSchema for TableSchemaRelation<Body> {
    type InsertArguments = Body;
    type InsertReturn = ();
    fn insert<'a>(
        loader: &mut Loader<'a, Self>,
        input: Self::InsertArguments,
    ) -> Self::InsertReturn {
        let values = input.into_values();
        loader.add_values(values);
    }
    type Read = Body;
    fn egglog_function_config(eg: &EGraph, name: String) -> FunctionConfig {
        let schema = Body::egglog_column_ty(eg);
        FunctionConfig {
            schema,
            default: egglog_bridge::DefaultVal::Const(Value::new_const(0)),
            merge: egglog_bridge::MergeFn::AssertEq,
            name,
            can_subsume: false,
        }
    }
}

pub struct Loader<'a, S: TableSchema + ?Sized> {
    eg: &'a mut EGraph,
    _p: PhantomData<S>,
    id: FunctionId,
    values: Vec<(FunctionId, Vec<Value>)>,
}

impl<'a, S: TableSchema + ?Sized> Loader<'a, S> {
    fn add_values(&mut self, values: Vec<Value>) {
        self.values.push((self.id, values));
    }
    pub fn add(&mut self, value: S::InsertArguments) -> S::InsertReturn {
        S::insert(self, value)
    }
    pub fn flush(&mut self) {
        self.eg.inner.add_values(std::mem::take(&mut self.values));
    }
}
impl<'a, S: TableSchema + ?Sized> Drop for Loader<'a, S> {
    fn drop(&mut self) {
        assert!(self.values.is_empty())
    }
}

impl EGraph {
    pub fn add_constructor<In: TokenTuple, Out: TokenValueOpaqueMarker>(
        &mut self,
        name: impl ToString,
    ) -> TableToken<TableSchemaConstructor<In, Out>>
    where
        In::Push<Out>: TokenTuple,
    {
        self.add_table::<TableSchemaConstructor<In, Out>>(name)
    }
    pub fn add_function<In: TokenTuple, Out: Token>(
        &mut self,
        name: impl ToString,
    ) -> TableToken<TableSchemaFunction<In, Out>>
    where
        In::Push<Out>: TokenTuple,
    {
        self.add_table::<TableSchemaFunction<In, Out>>(name)
    }
    pub fn add_relation<Body: TokenTuple>(
        &mut self,
        name: impl ToString,
    ) -> TableToken<TableSchemaRelation<Body>> {
        self.add_table::<TableSchemaRelation<Body>>(name)
    }
    fn add_table<S: TableSchema>(&mut self, name: impl ToString) -> TableToken<S> {
        let config = S::egglog_function_config(self, name.to_string());
        let id = self.inner.add_table(config);
        let _p = PhantomData;
        TableToken { id, _p }
    }

    pub fn loader<S: TableSchema>(&mut self, table: TableToken<S>) -> Loader<'_, S> {
        Loader {
            eg: self,
            _p: PhantomData,
            id: table.id,
            values: vec![],
        }
    }
    pub fn add_value<S: TableSchema>(
        &mut self,
        table: TableToken<S>,
        value: S::InsertArguments,
    ) -> S::InsertReturn {
        let mut loader = self.loader(table);
        let r = loader.add(value);
        loader.flush();
        r
    }

    pub fn for_each_row<S: TableSchema>(&self, table: TableToken<S>, mut f: impl FnMut(S::Read)) {
        self.inner
            .for_each(table.id, |entry| f(S::Read::from_values(entry.vals)));
    }
}
