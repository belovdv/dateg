use std::collections::HashSet;

use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::{
    Expr, Ident, Token,
    parse::{Parse, ParseStream},
    parse_macro_input,
    spanned::Spanned,
};

#[proc_macro]
pub fn rule(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = parse_macro_input!(input as RuleInput);

    let eg = &input.eg;
    let rb = Ident::new("rb", eg.span());
    let mut emitter = Emitter {
        rb: rb.clone(),
        action_span: rb.span(),
        counter: 0,
        variables: Default::default(),
        output: quote! {},
    };
    let actions = input.actions.iter();
    let actions = match actions.map(|a| emitter.emit_action(a)).collect() {
        Ok(()) => emitter.output,
        Err(err) => return err.into(),
    };

    quote! {{
        let mut #rb = #eg.rule_builder(None);
        #actions
        let id = #rb.build();
        #eg.add_ruleset_rule(id);
        id
    }}
    .into()
}

// Input

struct RuleInput {
    eg: Expr,
    actions: Vec<Action>,
}
struct Action {
    action: Ident,
    lhs: SExpr,
    rhs: SExpr,
}
enum SExpr {
    Leaf(Ident),
    Custom(Expr),
    Nested(Ident, Vec<Self>),
}

impl Parse for RuleInput {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let eg: Expr = input.parse()?;
        input.parse::<Token![;]>()?;
        let mut actions = Vec::new();
        while !input.is_empty() {
            actions.push(input.parse()?);
        }
        Ok(RuleInput { eg, actions })
    }
}
impl Parse for Action {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let content;
        syn::parenthesized!(content in input);
        let action = content.parse()?;
        let lhs = content.parse()?;
        let rhs = content.parse()?;
        Ok(Action { action, lhs, rhs })
    }
}
impl Parse for SExpr {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        Ok(if input.peek(syn::token::Brace) {
            let content;
            syn::braced!(content in input);
            SExpr::Custom(content.parse()?)
        } else if input.peek(syn::token::Paren) {
            let content;
            syn::parenthesized!(content in input);
            let f: Ident = content.parse()?;
            let mut args = Vec::new();
            while !content.is_empty() {
                args.push(content.parse()?);
            }
            SExpr::Nested(f, args)
        } else {
            SExpr::Leaf(input.parse()?)
        })
    }
}

// Output

struct Emitter {
    rb: Ident,
    action_span: Span,
    counter: usize,
    variables: HashSet<Ident>,
    output: TokenStream,
}

type Result<T> = std::result::Result<T, TokenStream>;
impl Emitter {
    fn emit(&mut self, code: TokenStream) {
        let output = std::mem::take(&mut self.output);
        self.output = quote! { #output #code };
    }

    fn emit_action(&mut self, action: &Action) -> Result<()> {
        let Action { action, lhs, rhs } = action;
        self.action_span = action.span();
        self.emit(quote! { #[cfg(false)] struct #action; });
        let rb = self.rb.clone();
        match action.to_string().as_str() {
            s if let Some(kind) = Kind::from_str(s) => {
                let bind = lhs.as_ident()?;
                let (f, args) = rhs.as_app()?;
                self.emit_atom(kind, bind, f, args)?;
            }
            "uni" => {
                let lhs = self.emit_arg(Kind::Add, lhs)?;
                let rhs = self.emit_arg(Kind::Add, rhs)?;
                let uni = Ident::new("union", self.action_span);
                self.emit(quote! { #rb.#uni(#lhs, #rhs); });
            }
            a => {
                let msg = format!("unknown action `{a}`");
                return Err(syn::Error::new(action.span(), msg).into_compile_error());
            }
        }
        Ok(())
    }

    fn emit_atom(&mut self, kind: Kind, bind: &Ident, f: &Ident, args: &[SExpr]) -> Result<()> {
        self.emit(quote! { #[cfg(false)] fn #f() {} });
        let args = self.emit_args(kind.args_handling(), args)?;
        let args_tuple = Ident::new("args", f.span());
        let rb = self.rb.clone();
        self.emit(quote! { let #args_tuple = (#(#args,)*); });
        if matches!(kind, Kind::Query) {
            self.maybe_init_var(bind, kind).unwrap();
        }
        let action = Ident::new(kind.to_str(), self.action_span);
        match kind {
            Kind::Add => {
                self.emit(quote! { let #bind = dateg::Entry::Var(#rb.#action(#f, #args_tuple)); })
            }
            Kind::Call => {
                self.emit(quote! { let #bind = dateg::Entry::Var(#rb.#action(#f, #args_tuple)); })
            }
            _ => self.emit(quote! { #rb.#action(#f, #args_tuple, #bind); }),
        }
        Ok(())
    }

    fn emit_args(&mut self, kind: Kind, args: &[SExpr]) -> Result<Vec<TokenStream>> {
        let mut r = vec![];
        for arg in args {
            r.push(self.emit_arg(kind, arg)?);
        }
        Ok(r)
    }
    fn emit_arg(&mut self, kind: Kind, arg: &SExpr) -> Result<TokenStream> {
        match arg {
            SExpr::Leaf(ident) => {
                self.maybe_init_var(ident, kind)?;
                Ok(quote! { #ident })
            }
            SExpr::Custom(expr) => Ok(quote! { dateg::Entry::Const(#expr) }),
            SExpr::Nested(f, args) => {
                let tmp = self.new_tmp_var_ident(f.span());
                self.emit_atom(kind, &tmp, f, args)?;
                Ok(quote! { #tmp })
            }
        }
    }

    fn maybe_init_var(&mut self, var: &Ident, kind: Kind) -> Result<()> {
        if self.variables.insert(var.clone()) {
            if !matches!(kind, Kind::Query) {
                return Err(syn::Error::new(var.span(), format!("var was not defined"))
                    .into_compile_error());
            }
            let rb = self.rb.clone();
            self.emit(quote! { let #var = dateg::Entry::Var(#rb.var_named(stringify!(#var))); });
        }
        Ok(())
    }
    fn new_tmp_var_ident(&mut self, span: Span) -> Ident {
        self.counter += 1;
        Ident::new(&format!("__tmp{}", self.counter), span)
    }
}

#[derive(Clone, Copy)]
enum Kind {
    Query,
    Add,
    Set,
    Call,
}
impl Kind {
    fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "query" => Self::Query,
            "add" => Self::Add,
            "set" => Self::Set,
            "call" => Self::Call,
            _ => return None,
        })
    }
    fn to_str(self) -> &'static str {
        match self {
            Self::Query => "query",
            Self::Add => "add",
            Self::Set => "set",
            Self::Call => "call",
        }
    }
    fn args_handling(self) -> Self {
        match self {
            Self::Query => self,
            _ => Self::Add,
        }
    }
}

impl SExpr {
    fn as_ident(&self) -> Result<&Ident> {
        let span = match self {
            Self::Leaf(ident) => return Ok(ident),
            Self::Custom(expr) => expr.span(),
            Self::Nested(f, _) => f.span(),
        };
        Err(syn::Error::new(span, format!("expected ident")).into_compile_error())
    }
    fn as_app(&self) -> Result<(&Ident, &[Self])> {
        let span = match self {
            Self::Leaf(ident) => ident.span(),
            Self::Custom(expr) => expr.span(),
            Self::Nested(f, args) => return Ok((f, args)),
        };
        Err(syn::Error::new(span, format!("expected app")).into_compile_error())
    }
}
