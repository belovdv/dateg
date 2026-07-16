use super::*;

const CFG: Config = Config::Tree;

pub fn emit(input: &Input) -> Result<TokenStream> {
    let Input { index, enums } = input;

    let fields = enums.iter().map(|e| {
        let field = &e.field;
        let map = e.map(CFG);
        quote! { #field: #map, }
    });

    let per_datatype = enums
        .iter()
        .map(|e| {
            let field = &e.field;
            let ty = &e.ty;
            let constructors = &e.constructors;
            let token = gen_token(&e.egraph_value);
            let map = e.map(CFG);
            let per_constructor = constructors
                .iter()
                .map(|cs| {
                    let constructor = &cs.constructor;
                    let cost = cs.cost(CFG)?;
                    ensure!(cs.consumes.is_none(), cs.constructor.span());
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
                #[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
