mod parse;

use proc_macro2::{Span, TokenStream};
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
    input.emit(Cfg::Tree).unwrap_or_else(|e| e).into()
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
///             { |inputs| cost_expression(using index on opaque inputs) }
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
    input.emit(Cfg::Dag).unwrap_or_else(|e| e).into()
}

type Result<T> = std::result::Result<T, TokenStream>;

#[derive(Clone, Copy)]
enum Cfg {
    Tree,
    Dag,
}

struct Input {
    index: Ident,
    enums: Vec<Enum>,
    containers: Vec<Ident>,
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
macro_rules! ensure {
    ($cond:expr, $span:expr, $msg:literal $($extra:tt)*) => {
        if !$cond {
            return err!($span, $msg $($extra)*);
        }
    };
    ($cond:expr, $span:expr) => {
        if !$cond {
            return err!($span, "{}", stringify!($cond));
        }
    };
}

impl Input {
    fn emit(&self, cfg: Cfg) -> Result<TokenStream> {
        let Self {
            index,
            enums,
            containers,
        } = self;

        let module = match cfg {
            Cfg::Tree => Ident::new("tree", Span::mixed_site()),
            Cfg::Dag => Ident::new("dag", Span::mixed_site()),
        };

        let fields = enums.iter().map(|e| {
            let field = &e.field;
            let map = e.map_ty(cfg);
            quote! { #field: #map, }
        });

        let per_datatype = enums
            .iter()
            .map(|e| {
                let field = &e.field;
                let ty = &e.ty;
                let constructors = &e.constructors;
                let token = gen_token(&e.egraph_value);
                let map = e.map_ty(cfg);
                let per_constructor = constructors
                    .iter()
                    .map(|cs| {
                        let constructor = &cs.constructor;
                        let fn_cost = cs.fn_cost(cfg)?;
                        let args_ = gen_unique_ids(&cs.args, "t");
                        let args = cs.args.iter().map(gen_token);
                        let mut consumes = None;
                        if let Some(closure) = &cs.consumes {
                            ensure!(matches!(cfg, Cfg::Dag), cs.constructor.span());
                            ensure_closure_is_simple(closure)?;
                            ensure!(closure.inputs.len() == 1, closure.span());
                            let inputs = closure.inputs.get(0).unwrap();
                            let body = &closure.body;
                            consumes = Some(quote! {
                                fn consumes(
                                    #inputs: Self::Inputs
                                ) -> impl IntoIterator<Item = dateg::Value> { #body }
                            });
                        }
                        Ok(quote! {
                            pub struct #constructor;
                            impl dateg_extractors::#module::Constructor for #constructor {
                                type Inputs = (#(#args,)*);
                                type Output = #token;
                                type Enum = #ty;
                                fn into_variant(inputs: Self::Inputs) -> Self::Enum {
                                    let (#(#args_,)*) = inputs;
                                    #ty::#constructor(#(#args_),*)
                                }
                                type Index = #index;
                                #fn_cost
                                #consumes
                            }
                        })
                    })
                    .collect::<Result<Vec<_>>>()?;
                let constructors = constructors.iter().map(|cs| {
                    let constructor = &cs.constructor;
                    let args = cs.args.iter().map(gen_token);
                    quote! { #constructor(#(#args),*) }
                });
                Ok(quote! {
                    impl dateg_extractors::#module::IndexFor<#token> for #index {
                        type Enum = #ty;
                        fn get_map(&self) -> &#map              { &self.#field }
                        fn get_map_mut(&mut self) -> &mut #map  { &mut self.#field }
                    }
                    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
                    pub enum #ty { #(#constructors,)* }
                    #(#per_constructor)*
                })
            })
            .collect::<Result<Vec<_>>>()?;

        let extract_args = enums.iter().enumerate().map(|(n, e)| {
            let css = e.constructors.iter().map(|cs| &cs.constructor);
            let names = gen_unique_ids(css, &format!("t{n}_"));
            let types = e.constructors.iter().map(|cs| {
                let inputs = cs.args.iter().map(gen_token);
                let output = gen_token(&e.egraph_value);
                quote! {
                    dateg::Table<((#(#inputs,)*), #output, dateg::True)>
                }
            });
            quote! { (#(#names),*): (#(#types),*), }
        });
        let extract_args = match cfg {
            Cfg::Tree => quote! {
                eg: &dateg::EGraph, #(#extract_args)*
            },
            Cfg::Dag => quote! {
                eg: &dateg::EGraph, root: impl dateg::TokenOpaqueMarker, #(#extract_args)*
            },
        };
        let extract_args2 = extract_args.clone();
        let extract_args1 = extract_args;
        let extract_set = enums
            .iter()
            .enumerate()
            .map(|(n, e)| {
                let css = e.constructors.iter().map(|cs| &cs.constructor);
                let names = gen_unique_ids(css, &format!("t{n}_"));
                e.constructors.iter().zip(names).map(|(cs, name)| {
                    let cs = &cs.constructor;
                    quote! { r.set_constructor::<#cs, _>(#name); }
                })
            })
            .flatten()
            .chain(containers.iter().filter_map(|c| {
                matches!(cfg, Cfg::Dag).then(|| quote! { r.set_container::<#c>(); })
            }));
        let extract_set2 = extract_set.clone();
        let extract_set1 = extract_set;

        let extract_args = match cfg {
            Cfg::Tree => quote! { eg },
            Cfg::Dag => quote! { eg, root },
        };

        Ok(quote! {
            #[derive(Default)]
            pub struct #index {
                #(#fields)*
            }
            #(#per_datatype)*
            impl #index {
                pub fn extractor(#extract_args1) -> dateg_extractors::#module::Extractor::<Self> {
                    let mut r = dateg_extractors::#module::Extractor::<Self>::default();
                    #(#extract_set1)*
                    r
                }
                pub fn extract(#extract_args2) -> Self {
                    let mut r = dateg_extractors::#module::Extractor::<Self>::default();
                    #(#extract_set2)*
                    r.extract(#extract_args)
                }
            }
        })
    }
}

impl Enum {
    fn map_ty(&self, cfg: Cfg) -> TokenStream {
        let ty = &self.ty;
        let token = gen_token(&self.egraph_value);
        match cfg {
            Cfg::Tree => quote! { dateg_extractors::AHashMap<#token, (usize, Vec<#ty>)> },
            Cfg::Dag => quote! { dateg_extractors::AHashMap<#token, #ty> },
        }
    }
}

impl Constructor {
    fn fn_cost(&self, cfg: Cfg) -> Result<TokenStream> {
        let (input, body) = match (&self.cost, cfg) {
            (None, Cfg::Tree) => {
                let args = gen_unique_ids(&self.args, "t");
                let body = quote! {{
                    let mut r = 1;
                    #[allow(non_snake_case)]
                    let (#(#args,)*) = inputs;
                    use dateg_extractors::tree::CostFor;
                    #( r += index.cost(#args, eg)?; )*
                    Some(r)
                }};
                let input =
                    quote! { inputs: Self::Inputs, index: &Self::Index, eg: &dateg::EGraph };
                (input, body)
            }
            (None, Cfg::Dag) => (
                quote! { inputs: Self::Inputs, eg: &dateg::EGraph },
                quote! { Some(1) },
            ),
            (Some(closure), _) => {
                ensure_closure_is_simple(closure)?;
                let input = match cfg {
                    Cfg::Tree => {
                        ensure!(closure.inputs.len() == 3, closure.span());
                        let inputs = closure.inputs.get(0).unwrap();
                        let index = closure.inputs.get(1).unwrap();
                        let eg = closure.inputs.get(2).unwrap();
                        quote! { #inputs: Self::Inputs, #index: &Self::Index, #eg: &dateg::EGraph }
                    }
                    Cfg::Dag => {
                        ensure!(closure.inputs.len() == 2, closure.span());
                        let inputs = closure.inputs.get(0).unwrap();
                        let eg = closure.inputs.get(1).unwrap();
                        quote! { #inputs: Self::Inputs, #eg: &dateg::EGraph }
                    }
                };
                (input, closure.body.clone().into_token_stream())
            }
        };
        Ok(quote! { #[allow(unused)] fn cost(#input) -> Option<usize> { #body } })
    }
}

fn gen_token(egraph_value: &Ident) -> TokenStream {
    quote_spanned! { egraph_value.span() => <#egraph_value as dateg::EGraphValue>::Token }
}
fn gen_unique_ids<S: Spanned>(iter: impl IntoIterator<Item = S>, prefix: &str) -> Vec<Ident> {
    let new = |(n, item): (usize, S)| Ident::new(&format!("{prefix}{n}"), item.span());
    iter.into_iter().enumerate().map(new).collect()
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
