mod dag;
mod parse;
mod tree;

use proc_macro2::TokenStream;
use quote::{ToTokens, quote, quote_spanned};
use syn::{
    ExprClosure, Ident, ReturnType, Token, custom_keyword,
    parse::{Parse, ParseStream},
    parse_macro_input,
    spanned::Spanned,
};

/// Defines list of constructors to select values from and index, capable to keep result
///
/// Usage (see tests for details):
/// ```no_run
/// # macro_rules! index_tree { ($($tt:tt)*) => { () }; };
/// index_tree!(IndexStructName
///     index_field1: TypeForEnumOfResultsName1 (datatype EGraphValueOpaque1
///         Constructor1 (Arg1 Arg2 Arg3)
///         Constructor2 () { |inputs, index| cost_expression(using index on opaque inputs) }
///     )
/// )
/// ```
///
/// Generates:
/// - `struct Index { index_field: Map<EGraphValueOpaque1, (usize, Vec<EnumOfConstructors>)> }`
///     - finds all versions, first is accessible via `value` and is guaranteed to be acyclic
/// - `enum EnumOfConstructors { Constructor(Arg, ...), ... } ...`
/// - trait implementations
/// - `fn Index::extract(&EGraph, table tuples) -> Self`
#[proc_macro]
pub fn index_tree(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = parse_macro_input!(input as Input);
    tree::emit(&input).unwrap_or_else(|e| e).into()
}

/// Defines list of constructors to select values from and index, capable to keep result
///
/// Usage (see tests for details):
/// ```no_run
/// # macro_rules! index_tree { ($($tt:tt)*) => { () }; };
/// index_tree!(IndexStructName
///     index_field1: TypeForEnumOfResultsName1 (datatype EGraphValueOpaque1
///         Constructor1 (Arg1 Arg2 Arg3)
///         Constructor2 ()
///             { |inputs, index| cost_expression(using index on opaque inputs) }
///             { |inputs| selection of opaque inputs to be considered consumed }
///     )
/// )
/// ```
///
/// Generates:
/// - `struct Index { index_field: Map<EGraphValueOpaque1, EnumOfConstructors> }`
/// - `enum EnumOfConstructors { Constructor(Arg, ...), ... } ...`
/// - trait implementations
/// - `fn Index::extract(&EGraph, TokenOpaque, table tuples) -> Self`
#[proc_macro]
pub fn index_dag(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = parse_macro_input!(input as Input);
    dag::emit(&input).unwrap_or_else(|e| e).into()
}

type Result<T> = std::result::Result<T, TokenStream>;

#[derive(Clone, Copy)]
enum Config {
    Tree,
    Dag,
}

struct Input {
    index: Ident,
    enums: Vec<Enum>,
}
struct Enum {
    field: Ident,
    ty: Ident,
    egraph_value: Ident,
    constructors: Vec<Constructor>,
}
struct Constructor {
    constructor: Ident,
    args: Vec<Ident>,
    cost: Option<ExprClosure>,
    /// Dag-extractor only
    consumes: Option<ExprClosure>,
}

macro_rules! err {
    ($span:expr, $msg:literal $($extra:tt)*) => {
        Err(syn::Error::new($span, format!($msg $($extra)*)).into_compile_error())
    };
}
use err;
macro_rules! ensure {
    ($cond:expr, $span:expr, $msg:literal $($extra:tt)*) => {
        if !$cond {
            return err!($span, $msg $($extra)*);
        }
    };
    ($cond:expr, $span:expr) => {
        if !$cond {
            return err!($span, "{}", stringify!($expr));
        }
    };
}
use ensure;

impl Enum {
    fn map(&self, cfg: Config) -> TokenStream {
        let ty = &self.ty;
        let token = gen_token(&self.egraph_value);
        match cfg {
            Config::Tree => quote! { dateg_extractors::AHashMap<#token, (usize, Vec<#ty>)> },
            Config::Dag => quote! { dateg_extractors::AHashMap<#token, #ty> },
        }
    }
}

impl Constructor {
    fn cost(&self, cfg: Config) -> Result<TokenStream> {
        let (input, body) = match &self.cost {
            Some(closure) => {
                ensure_closure_is_simple(closure)?;
                let input = match cfg {
                    Config::Tree => {
                        ensure!(closure.inputs.len() == 2, closure.span());
                        let inputs = closure.inputs.get(0).unwrap();
                        let index = closure.inputs.get(1).unwrap();
                        quote! { #inputs: Self::Inputs, #index: &Self::Index }
                    }
                    Config::Dag => {
                        ensure!(closure.inputs.len() == 1, closure.span());
                        let inputs = closure.inputs.get(0).unwrap();
                        quote! { #inputs: Self::Inputs }
                    }
                };
                (input, closure.body.clone().into_token_stream())
            }
            None => match cfg {
                Config::Tree => {
                    let inputs = Ident::new("inputs", self.constructor.span());
                    let index = Ident::new("index", self.constructor.span());
                    let args = generate_unique_identifiers(&self.args, "t");
                    let body = quote! {{
                        let mut r = 1;
                        #[allow(non_snake_case)]
                        let (#(#args,)*) = #inputs;
                        use dateg_extractors::tree::CostFor;
                        #( r += #index.cost(#args)?; )*
                        Some(r)
                    }};
                    let input = quote! { #inputs: Self::Inputs, #index: &Self::Index };
                    (input, body)
                }
                Config::Dag => (quote! { _: Self::Inputs }, quote! { Some(1) }),
            },
        };
        Ok(quote! { #[allow(unused)] fn cost(#input) -> Option<usize> { #body } })
    }
}

fn gen_token(egraph_value: &Ident) -> TokenStream {
    quote_spanned! { egraph_value.span() => <#egraph_value as dateg::EGraphValue>::Token }
}
fn generate_unique_identifiers<S: Spanned>(
    iter: impl IntoIterator<Item = S>,
    prefix: &str,
) -> Vec<Ident> {
    let mut r = vec![];
    for item in iter {
        r.push(Ident::new(&format!("{prefix}{}", r.len()), item.span()));
    }
    r
}

fn ensure_closure_is_simple(closure: &ExprClosure) -> Result<()> {
    ensure!(closure.attrs.is_empty(), closure.span());
    ensure!(closure.lifetimes.is_none(), closure.span());
    ensure!(closure.constness.is_none(), closure.span());
    ensure!(closure.movability.is_none(), closure.span());
    ensure!(closure.asyncness.is_none(), closure.span());
    ensure!(closure.capture.is_none(), closure.span());
    ensure!(
        matches!(closure.output, ReturnType::Default),
        closure.span()
    );
    Ok(())
}
