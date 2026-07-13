use std::hash::Hash;

use egglog_core_relations::Value;

use crate::*;

pub trait Bool: Copy + Eq + Hash + Send + Sync + 'static {
    const VAL: bool;
}
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct True;
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct False;
impl Bool for True {
    const VAL: bool = true;
}
impl Bool for False {
    const VAL: bool = false;
}

pub fn token_unit() -> TokenPrimitive<()> {
    use std::sync::OnceLock;
    static TOKEN_UNIT: OnceLock<TokenPrimitive<()>> = OnceLock::new();
    *TOKEN_UNIT.get_or_init(|| TokenPrimitive::from_egglog(Value::new_const(0)))
}
