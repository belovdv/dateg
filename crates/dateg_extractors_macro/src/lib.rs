use proc_macro2::TokenStream;
use quote::{ToTokens, quote, quote_spanned};
use syn::{
    ExprClosure, Ident, ReturnType, Token, custom_keyword,
    parse::{Parse, ParseStream},
    parse_macro_input,
    spanned::Spanned,
};

#[proc_macro]
pub fn index_tree(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = parse_macro_input!(input as ExtractorTreeInput);
    emit(&input).unwrap_or_else(|e| e).into()
}

// Utils

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
            return err!($span, "{}", stringify!($expr));
        }
    };
}

// Input

struct ExtractorTreeInput {
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
}

impl Parse for ExtractorTreeInput {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let index = input.parse()?;
        let mut enums = vec![];
        while !input.is_empty() {
            enums.push(input.parse()?);
        }
        Ok(Self { index, enums })
    }
}
impl Parse for Enum {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let field = input.parse()?;
        input.parse::<Token![:]>()?;
        let ty = input.parse()?;
        let content;
        syn::parenthesized!(content in input);
        custom_keyword!(datatype);
        content.parse::<datatype>()?;
        let egraph_value = content.parse()?;
        let mut constructors = vec![];
        while !content.is_empty() {
            constructors.push(content.parse()?);
        }
        Ok(Self {
            field,
            ty,
            egraph_value,
            constructors,
        })
    }
}
impl Parse for Constructor {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let constructor = input.parse()?;
        let content;
        syn::parenthesized!(content in input);
        let mut args = vec![];
        while !content.is_empty() {
            args.push(content.parse()?);
        }
        let mut cost = None;
        if input.peek(syn::token::Brace) {
            let content;
            syn::braced!(content in input);
            cost = Some(content.parse()?);
        }
        Ok(Self {
            constructor,
            args,
            cost,
        })
    }
}

// Output

type Result<T> = std::result::Result<T, TokenStream>;

fn emit(input: &ExtractorTreeInput) -> Result<TokenStream> {
    let ExtractorTreeInput { index, enums } = input;

    let fields = enums.iter().map(|e| {
        let Enum { field, ty, .. } = e;
        let token = gen_token(&e.egraph_value);
        quote! { #field: dateg_extractors::AHashMap<#token, (usize, Vec<#ty>)>, }
    });

    let per_datatype = enums
        .iter()
        .map(|e| {
            let field = &e.field;
            let ty = &e.ty;
            let constructors = &e.constructors;
            let token = gen_token(&e.egraph_value);
            let map = e.map();
            let per_constructor = constructors
                .iter()
                .map(|cs| {
                    let constructor = &cs.constructor;
                    let cost = cs.cost()?;
                    let args_ = generate_unique_identifiers(&cs.args, "t");
                    let args = cs.args.iter().map(gen_token);
                    Ok(quote! {
                        pub struct #constructor;
                        impl dateg_extractors::tree::Constructor for #constructor {
                            type Inputs = (#(#args,)*);
                            type Output = #token;
                            type Enum = #ty;
                            fn into_variant(inputs: Self::Inputs) -> Self::Enum {
                                let (#(#args_,)*) = inputs;
                                #ty::#constructor(#(#args_),*)
                            }
                            type Index = #index;
                            #cost
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
                impl dateg_extractors::tree::IndexFor<#token> for #index {
                    type Enum = #ty;
                    fn get_map(&self) -> &#map {
                        &self.#field
                    }
                    fn get_map_mut(&mut self) -> &mut #map {
                        &mut self.#field
                    }
                }
                #[derive(Clone, Copy, PartialEq, Eq)]
                pub enum #ty {
                    #(#constructors,)*
                }
                #(#per_constructor)*
            })
        })
        .collect::<Result<Vec<_>>>()?;

    let extract_args = enums.iter().enumerate().map(|(n, e)| {
        let css = e.constructors.iter().map(|cs| &cs.constructor);
        let names = generate_unique_identifiers(css, &format!("t{n}_"));
        let types = e.constructors.iter().map(|cs| {
            let inputs = cs.args.iter().map(gen_token);
            let output = gen_token(&e.egraph_value);
            quote! {
                dateg::Table<((#(#inputs,)*), #output, dateg::True)>
            }
        });
        quote! { (#(#names),*): (#(#types),*), }
    });
    let extract_set = enums
        .iter()
        .enumerate()
        .map(|(n, e)| {
            let css = e.constructors.iter().map(|cs| &cs.constructor);
            let names = generate_unique_identifiers(css, &format!("t{n}_"));
            e.constructors.iter().zip(names).map(|(cs, name)| {
                let cs = &cs.constructor;
                quote! { r.set_constructor::<#cs, _>(#name); }
            })
        })
        .flatten();

    Ok(quote! {
        #[derive(Default)]
        pub struct #index {
            #(#fields)*
        }
        #(#per_datatype)*
        impl #index {
            pub fn extract(eg: &dateg::EGraph, #(#extract_args)*) -> Self {
                let mut r = dateg_extractors::tree::Extractor::<Self>::default();
                #(#extract_set)*
                r.extract(eg)
            }
        }
    })
}

impl Enum {
    fn map(&self) -> TokenStream {
        let ty = &self.ty;
        let token = gen_token(&self.egraph_value);
        quote! { dateg_extractors::AHashMap<#token, (usize, Vec<#ty>)> }
    }
}
impl Constructor {
    fn cost(&self) -> Result<TokenStream> {
        let constructor = &self.constructor;
        let (attrs, inputs, index, body) = match &self.cost {
            Some(closure) => {
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
                ensure!(closure.inputs.len() == 2, closure.span());
                let inputs = closure.inputs.get(0).unwrap();
                let index = closure.inputs.get(1).unwrap();
                (
                    quote! {},
                    inputs.clone().into_token_stream(),
                    index.clone().into_token_stream(),
                    closure.body.clone().into_token_stream(),
                )
            }
            None => {
                let inputs = Ident::new("inputs", constructor.span());
                let index = Ident::new("index", constructor.span());
                let args = generate_unique_identifiers(&self.args, "t");
                let body = quote! {{
                    let mut r = 1;
                    #[allow(non_snake_case)]
                    let (#(#args,)*) = #inputs;
                    use dateg_extractors::tree::CostFor;
                    #( r += #index.cost(#args)?; )*
                    Some(r)
                }};
                let attrs = quote! { #[allow(unused)] };
                (
                    attrs,
                    inputs.into_token_stream(),
                    index.into_token_stream(),
                    body,
                )
            }
        };
        Ok(quote! {
            #attrs
            fn cost(#inputs: Self::Inputs, #index: &Self::Index) -> Option<usize> {
                #body
            }
        })
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
