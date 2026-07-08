//! Schema: description of table, defining signature of methods, working with it

use std::marker::PhantomData;

use egglog_bridge::{DefaultVal, FunctionConfig, MergeFn};
use egglog_core_relations::Value;

use crate::*;

pub trait DefaultValT {
    fn egglog(eg: &egglog_bridge::EGraph) -> DefaultVal;
}
pub trait MergeFnT {
    fn egglog(eg: &egglog_bridge::EGraph) -> MergeFn;
}
macro_rules! into_type {
    ($Egglog:ident $Variant:ident) => {
        paste::paste! {
            pub struct [< $Egglog $Variant >];
            impl [< $Egglog T >] for [< $Egglog $Variant >] {
                fn egglog(_: &egglog_bridge::EGraph) -> $Egglog { $Egglog::$Variant }
            }
        }
    };
}
into_type!(DefaultVal FreshId);
into_type!(DefaultVal Fail);
into_type!(MergeFn UnionId);
into_type!(MergeFn AssertEq);
pub struct DefaultValConst0;
impl DefaultValT for DefaultValConst0 {
    fn egglog(_: &egglog_bridge::EGraph) -> DefaultVal {
        egglog_bridge::DefaultVal::Const(Value::new_const(0))
    }
}
pub fn token_unit() -> TokenValuePrimitive<()> {
    use std::sync::OnceLock;
    static TOKEN_UNIT: OnceLock<TokenValuePrimitive<()>> = OnceLock::new();
    *TOKEN_UNIT.get_or_init(|| TokenValuePrimitive::from_value(Value::new_const(0)))
}

pub trait Schema: 'static {
    type Inputs: TokenTuple;
    type Output: Token;

    type DefaultVal: DefaultValT;
    type MergeFn: MergeFnT;

    fn egglog(eg: &egglog_bridge::EGraph, name: impl ToString) -> FunctionConfig {
        let mut schema = Self::Inputs::egglog(eg);
        schema.push(Self::Output::egglog(eg));
        FunctionConfig {
            schema,
            default: Self::DefaultVal::egglog(eg),
            merge: Self::MergeFn::egglog(eg),
            name: name.to_string(),
            can_subsume: false,
        }
    }
}

pub struct TableConstructorSchema<In, Out>(PhantomData<(In, Out)>);
impl<In: TokenTuple, Out: TokenValueOpaqueMarker> Schema for TableConstructorSchema<In, Out> {
    type Inputs = In;
    type Output = Out;
    type DefaultVal = DefaultValFreshId;
    type MergeFn = MergeFnUnionId;
}
pub struct TableFunctionSchema<In, Out>(PhantomData<(In, Out)>);
impl<In: TokenTuple, Out: Token> Schema for TableFunctionSchema<In, Out> {
    type Inputs = In;
    type Output = Out;
    type DefaultVal = DefaultValFail;
    type MergeFn = MergeFnAssertEq;
}
pub struct TableRelationSchema<Types>(PhantomData<Types>);
impl<Types: TokenTuple> Schema for TableRelationSchema<Types> {
    type Inputs = Types;
    type Output = TokenValuePrimitive<()>;
    type DefaultVal = DefaultValConst0;
    type MergeFn = MergeFnAssertEq;
}
