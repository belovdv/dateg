use super::*;

impl Parse for Input {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let index = input.parse()?;
        let mut enums = vec![];
        while !input.is_empty() && !input.peek(syn::token::Bracket) {
            enums.push(input.parse()?);
        }
        let mut containers = vec![];
        if !input.is_empty() {
            let content;
            syn::bracketed!(content in input);
            while !content.is_empty() {
                containers.push(content.parse()?);
            }
        }
        Ok(Self {
            index,
            enums,
            containers,
        })
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
        let mut consumes = None;
        if input.peek(syn::token::Brace) {
            let content;
            syn::braced!(content in input);
            consumes = Some(content.parse()?);
        }
        Ok(Self {
            constructor,
            args,
            cost,
            consumes,
        })
    }
}
