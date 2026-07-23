use std::{hash::Hash, marker::PhantomData};

use egglog_bridge::ColumnTy;
use egglog_core_relations::{BaseValue, ContainerValue, Value};
use egglog_numeric_id::NumericId;

pub trait EGraphValue: 'static {
    type Token: Token<Value = Self>;
}

pub trait Token: std::fmt::Debug + Copy + Eq + Hash + Send + Sync + 'static {
    type Value: Send + Sync + 'static;
    fn from_egglog(egglog: Value) -> Self;
    fn into_egglog(self) -> Value;
    fn column_ty(eg: &egglog_bridge::EGraph) -> ColumnTy;
    fn as_non_primitive(self) -> Option<Value> {
        None
    }
}

// Primitive

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct TokenPrimitive<T: BaseValue>(Value, PhantomData<T>);
impl<T: BaseValue> EGraphValue for T {
    type Token = TokenPrimitive<Self>;
}

#[rustfmt::skip]
impl<T: BaseValue> Token for TokenPrimitive<T> {
    type Value = T;
    fn from_egglog(egglog: Value) -> Self               { Self(egglog, PhantomData) }
    fn into_egglog(self) -> Value                       { self.0 }
    fn column_ty(eg: &egglog_bridge::EGraph) -> ColumnTy {
        ColumnTy::Base(eg.base_values().get_ty::<T>())
    }
}

impl<T: BaseValue> TokenPrimitive<T> {
    /// Note: this can call [`T::clone`] internally
    pub fn get(&self, eg: &crate::EGraph) -> T {
        eg.inner.base_values().unwrap::<T>(self.0)
    }
}

pub trait TokenPrimitiveMarker: Token {}
impl<T: BaseValue> TokenPrimitiveMarker for TokenPrimitive<T> {}

// Opaque

pub struct TokenOpaque<T: Send + Sync + 'static>(Value, PhantomData<T>);

#[rustfmt::skip]
impl<T: Send + Sync+'static> Token for TokenOpaque<T> {
    type Value = T;
    fn from_egglog(egglog: Value) -> Self               { Self(egglog, PhantomData) }
    fn into_egglog(self) -> Value                       { self.0 }
    fn as_non_primitive(self) -> Option<Value>          { Some(self.0) }
    fn column_ty(_: &egglog_bridge::EGraph) -> ColumnTy { ColumnTy::Id }
}

impl<T: Send + Sync + 'static> TokenOpaque<T> {
    pub fn canon(self, eg: &crate::EGraph) -> Self {
        Self::from_egglog(eg.inner.get_canon_repr(self.0, ColumnTy::Id))
    }
}

pub trait TokenOpaqueMarker: Token {
    fn as_token_opaque(self) -> TokenOpaque<Self::Value>;
    fn canon(self, eg: &crate::EGraph) -> Self;
}
impl<T: Send + Sync + 'static> TokenOpaqueMarker for TokenOpaque<T> {
    fn as_token_opaque(self) -> TokenOpaque<Self::Value> {
        self
    }
    fn canon(self, eg: &crate::EGraph) -> Self {
        TokenOpaque::canon(self, eg)
    }
}

// Container

pub trait ContainerValueExt: ContainerValue {
    type Element: TokenOpaqueMarker;
    fn iter(&self) -> impl Iterator<Item = Self::Element> {
        ContainerValue::iter(self).map(Token::from_egglog)
    }
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct TokenContainer<T: ContainerValueExt>(Value, PhantomData<T>);
impl<T: ContainerValueExt> Copy for TokenContainer<T> {}

#[rustfmt::skip]
impl<T: ContainerValueExt> Token for TokenContainer<T> {
    type Value = T;
    fn from_egglog(egglog: Value) -> Self               { Self(egglog, PhantomData) }
    fn into_egglog(self) -> Value                       { self.0 }
    fn as_non_primitive(self) -> Option<Value>          { Some(self.0) }
    fn column_ty(_: &egglog_bridge::EGraph) -> ColumnTy { ColumnTy::Id }
}

impl<T: ContainerValueExt> TokenContainer<T> {
    pub fn canon(&self, eg: &crate::EGraph) -> Self {
        Self::from_egglog(eg.inner.get_canon_repr(self.0, ColumnTy::Id))
    }

    pub fn get<'eg>(&self, eg: &'eg crate::EGraph) -> impl std::ops::Deref<Target = T> + 'eg {
        eg.inner.container_values().get_val::<T>(self.0).unwrap()
    }
}

// Container Vec

pub struct ContainerVec<T: EGraphValue>(pub Vec<T::Token>)
where
    T::Token: TokenOpaqueMarker;

impl<T: EGraphValue> ContainerValue for ContainerVec<T>
where
    T::Token: TokenOpaqueMarker,
{
    fn rebuild_contents(&mut self, r: &dyn egglog_core_relations::ValueRebuilder) -> bool {
        let mut changed = false;
        for val in self.0.iter_mut() {
            let new = r.rebuild_val(val.into_egglog());
            changed |= new != val.into_egglog();
            *val = T::Token::from_egglog(new);
        }
        changed
    }
    fn iter(&self) -> impl Iterator<Item = Value> + '_ {
        self.0.iter().map(|t| t.into_egglog())
    }
}
impl<T: EGraphValue> ContainerValueExt for ContainerVec<T>
where
    T::Token: TokenOpaqueMarker,
{
    type Element = T::Token;
}

impl<T: EGraphValue> EGraphValue for ContainerVec<T>
where
    T::Token: TokenOpaqueMarker,
{
    type Token = TokenContainer<ContainerVec<T>>;
}

// Utils

impl<T: BaseValue> Copy for TokenPrimitive<T> {}

impl<T: Send + Sync + 'static> Clone for TokenOpaque<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone(), self.1.clone())
    }
}
impl<T: Send + Sync + 'static> Copy for TokenOpaque<T> {}
impl<T: Send + Sync + 'static> PartialEq for TokenOpaque<T> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0 && self.1 == other.1
    }
}
impl<T: Send + Sync + 'static> Eq for TokenOpaque<T> {}
impl<T: Send + Sync + 'static> Hash for TokenOpaque<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state);
        self.1.hash(state);
    }
}

impl<T: EGraphValue> Clone for ContainerVec<T>
where
    T::Token: TokenOpaqueMarker,
{
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}
impl<T: EGraphValue> PartialEq for ContainerVec<T>
where
    T::Token: TokenOpaqueMarker,
{
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}
impl<T: EGraphValue> Eq for ContainerVec<T> where T::Token: TokenOpaqueMarker {}
impl<T: EGraphValue> Hash for ContainerVec<T>
where
    T::Token: TokenOpaqueMarker,
{
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl<T: BaseValue> std::fmt::Debug for TokenPrimitive<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("{}({})", SN::<T>(PhantomData), self.0.index()))
    }
}
impl<T: Send + Sync + 'static> std::fmt::Debug for TokenOpaque<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("{}({})", SN::<T>(PhantomData), self.0.index()))
    }
}
impl<T: ContainerValueExt> std::fmt::Debug for TokenContainer<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("{}({})", SN::<T>(PhantomData), self.0.index()))
    }
}

// Copied from bevy https://docs.rs/disqualified/latest/src/disqualified/short_name.rs.html

struct SN<T>(PhantomData<T>);

impl<T> core::fmt::Display for SN<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let full_name = core::any::type_name::<T>();
        // Generics result in nested paths within <..> blocks.
        // Consider "bevy_render::camera::camera::extract_cameras<bevy_render::camera::bundle::Camera3d>".
        // To tackle this, we parse the string from left to right, collapsing as we go.
        let mut index: usize = 0;
        let end_of_string = full_name.len();

        while index < end_of_string {
            let rest_of_string = full_name.get(index..end_of_string).unwrap_or_default();

            // Collapse everything up to the next special character,
            // then skip over it
            if let Some(special_character_index) = rest_of_string.find(|c: char| {
                (c == ' ')
                    || (c == '<')
                    || (c == '>')
                    || (c == '(')
                    || (c == ')')
                    || (c == '[')
                    || (c == ']')
                    || (c == ',')
                    || (c == ';')
            }) {
                let segment_to_collapse = rest_of_string
                    .get(0..special_character_index)
                    .unwrap_or_default();

                f.write_str(collapse_type_name(segment_to_collapse))?;

                // Insert the special character
                let special_character =
                    &rest_of_string[special_character_index..=special_character_index];

                f.write_str(special_character)?;

                match special_character {
                    ">" | ")" | "]"
                        if rest_of_string[special_character_index + 1..].starts_with("::") =>
                    {
                        f.write_str("::")?;
                        // Move the index past the "::"
                        index += special_character_index + 3;
                    }
                    // Move the index just past the special character
                    _ => index += special_character_index + 1,
                }
            } else {
                // If there are no special characters left, we're done!
                f.write_str(collapse_type_name(rest_of_string))?;
                index = end_of_string;
            }
        }

        Ok(())
    }
}

#[inline(always)]
fn collapse_type_name(string: &str) -> &str {
    // Enums types are retained.
    // As heuristic, we assume the enum type to be uppercase.
    let mut segments = string.rsplit("::");
    let (last, second_last): (&str, Option<&str>) = (segments.next().unwrap(), segments.next());
    let Some(second_last) = second_last else {
        return last;
    };

    if second_last.starts_with(char::is_uppercase) {
        let index = string.len() - last.len() - second_last.len() - 2;
        &string[index..]
    } else {
        last
    }
}
