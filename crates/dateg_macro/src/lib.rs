use std::collections::HashSet;

use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::{
    Expr, Ident, Token,
    parse::{Parse, ParseStream},
    parse_macro_input,
    spanned::Spanned,
};

/// Constructs `rule` for `EGraph`.
///
/// Example (from `dateg` basic tests):
/// ```no_run
/// # macro_rules! rule { ($($tt:tt)*) => { () }; };
/// let rule_id = rule!(egraph;
///     (query len (add (path a b) (path b c)))
///     (set len (path a c))
///     (call (log_path_concat a b c len))
/// );
/// ```
///
/// Syntax: `rule!(<EGraph ref expr>; <action>*)`
///
/// Actions are symbolic expressions with nesting
/// - functional symbol can refer to table or, sometimes, function
/// - nested expressions are flattened using extra variable
/// - nested expressions use one of `query` or `add` basing on side of action
/// - leaf can be identifier (variable) or custom expression in braces (constant)
///
/// LHS Actions:
/// - `(query <result> (<table|func> <args>*))`: primarily (kinda only) lhs action
///     - `table`: creates variables if they weren't created before, queries database
///     - `function`: creates result variable if it wasn't created, binds it to evaluation of inputs
/// - `(contains (<table|func> <args>*)`: syntax sugar on top of `query` for cases with unit type
///
/// RHS Actions:
/// - `(add <result> (<table|func> <args>*))`: defines value
///     - `table`: gets or create existing value or creates new - can be used only with constructors
///     - `func`: evaluates function on arguments
/// - `(set <result> (<table> <args>*))`: set value, call merge procedure on conflict
/// - `(uni <a> <b>)`: unions two values (variables of opaque types)
/// - `(insert (<table> <args>*))`: syntax sugar on top of `set` for relations
/// - `(call (<func> <args>*))`: similar to `add`, but for return value unit (usually logging)
#[proc_macro]
pub fn rule(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = parse_macro_input!(input as RuleInput);

    let eg = &input.eg;
    let rb = Ident::new("rb", eg.span());
    let mut emitter = Emitter {
        eg,
        rb: rb.clone(),
        action_span: rb.span(),
        counter: 0,
        defined: Default::default(),
        output_pre: quote! {},
        output: quote! {},
    };
    let actions = input.actions.iter();
    let actions = match actions.map(|a| emitter.emit_action(a)).collect() {
        Ok(()) => emitter.output,
        Err(err) => return err.into(),
    };
    let output_pre = emitter.output_pre;

    quote! {{
        #output_pre
        let mut #rb = #eg.rule_builder(None);
        #actions
        let id = #rb.build();
        #eg.add_ruleset_rule(id);
        id
    }}
    .into()
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
}

// Input

struct RuleInput {
    eg: Expr,
    actions: Vec<Action>,
}
struct Action {
    action: Ident,
    args: Vec<SExpr>,
}
enum SExpr {
    Implicit(Span),
    Leaf(Ident),
    Value {
        val: Expr,
        primitive: bool,
        add_into: bool,
    },
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
        let mut args = vec![];
        while !content.is_empty() {
            args.push(content.parse()?);
        }
        Ok(Action { action, args })
    }
}
impl Parse for SExpr {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        Ok(if input.peek(syn::LitStr) {
            SExpr::Value {
                val: Expr::Lit(syn::ExprLit {
                    attrs: vec![],
                    lit: input.parse()?,
                }),
                primitive: true,
                add_into: true,
            }
        } else if input.peek(syn::Lit) {
            SExpr::Value {
                val: Expr::Lit(syn::ExprLit {
                    attrs: vec![],
                    lit: input.parse()?,
                }),
                primitive: true,
                add_into: false,
            }
        } else if input.peek(syn::token::Brace) {
            let content;
            syn::braced!(content in input);
            let mut primitive = false;
            if content.parse::<syn::Token![#]>().is_ok() {
                primitive = true;
            }
            SExpr::Value {
                val: content.parse()?,
                primitive,
                add_into: false,
            }
        } else if input.peek(syn::token::Paren) {
            let content;
            syn::parenthesized!(content in input);
            let f: Ident = content.parse()?;
            let mut args = Vec::new();
            while !content.is_empty() {
                args.push(content.parse()?);
            }
            SExpr::Nested(f, args)
        } else if input.peek(Token![_]) {
            let underscore: Token![_] = input.parse()?;
            SExpr::Implicit(underscore.span())
        } else {
            SExpr::Leaf(input.parse()?)
        })
    }
}

// Output

struct Emitter<'a> {
    eg: &'a Expr,
    rb: Ident,
    action_span: Span,
    counter: usize,
    defined: HashSet<Ident>,
    output_pre: TokenStream,
    output: TokenStream,
}

type Result<T> = std::result::Result<T, TokenStream>;
impl Emitter<'_> {
    fn emit(&mut self, code: TokenStream) {
        let output = std::mem::take(&mut self.output);
        self.output = quote! { #output #code };
    }
    fn emit_pre(&mut self, code: TokenStream) {
        let output_pre = std::mem::take(&mut self.output_pre);
        self.output_pre = quote! { #output_pre #code };
    }

    fn emit_action(&mut self, action: &Action) -> Result<()> {
        let Action { action, args } = action;
        self.action_span = action.span();
        self.emit(quote! { #[cfg(false)] struct #action; });
        let rb = self.rb.clone();
        match action.to_string().as_str() {
            s if let Some(kind) = Kind::from_str(s) => {
                ensure!(args.len() == 2, self.action_span, "expected 2 args");
                let bind = match &args[0] {
                    SExpr::Implicit(span) => self.new_ident(*span),
                    SExpr::Leaf(ident) => ident.clone(),
                    SExpr::Value {
                        val,
                        primitive,
                        add_into,
                    } => {
                        let mut expr = quote! { #val };
                        if *primitive {
                            let eg = self.eg;
                            let pre = self.new_ident(expr.span());
                            let into = add_into.then(|| quote! { .into() });
                            self.emit_pre(
                                quote! { let #pre = #eg.add_primitive_value(#expr #into); },
                            );
                            expr = quote! { #pre };
                        }
                        let val = self.new_ident(expr.span());
                        self.emit(quote! { let #val = dateg::Entry::Const(#expr); });
                        self.defined.insert(val.clone());
                        val
                    }
                    SExpr::Nested(ident, ..) => return err!(ident.span(), "expected leaf"),
                };
                let (f, args) = args[1].as_app()?;
                self.emit_atom(kind, &bind, f, args)?;
            }
            s if let Some(kind) = KindUnary::from_str(s) => {
                ensure!(args.len() == 1, self.action_span, "expected 1 arg");
                let unit = self.unit();
                let (f, args) = args[0].as_app()?;
                self.emit(quote! { #[cfg(false)] fn #f() {} });
                let args = self.emit_args(kind.args_handling(), args)?;
                let action = Ident::new(kind.to_str(), self.action_span);
                let args = quote::quote_spanned! { f.span() => (#(#args,)*) };
                match kind {
                    KindUnary::Contains | KindUnary::Insert => {
                        self.emit(quote! { #rb.#action(#f, #args, #unit); })
                    }
                    KindUnary::Call => {
                        self.emit(quote! { #rb.#action(#f, #args); });
                    }
                }
            }
            "uni" => {
                ensure!(args.len() == 2, self.action_span, "expected 2 args");
                let lhs = self.emit_arg(Kind::Add, &args[0])?;
                let rhs = self.emit_arg(Kind::Add, &args[1])?;
                let uni = Ident::new("union", self.action_span);
                self.emit(quote! { #rb.#uni(#lhs, #rhs); });
            }
            a => return err!(action.span(), "unknown action `{a}`"),
        }
        Ok(())
    }

    fn emit_atom(&mut self, kind: Kind, bind: &Ident, f: &Ident, args: &[SExpr]) -> Result<()> {
        self.emit(quote! { #[cfg(false)] fn #f() {} });
        let args = self.emit_args(kind.args_handling(), args)?;
        let rb = self.rb.clone();
        if matches!(kind, Kind::Query) {
            self.maybe_init_var(bind, kind).unwrap();
        }
        let action = Ident::new(kind.to_str(), self.action_span);
        let args = quote::quote_spanned! { f.span() => (#(#args,)*) };
        match kind {
            Kind::Add => {
                ensure!(
                    !self.defined.contains(bind),
                    bind.span(),
                    "this definition will shadow previous mentions, use different name"
                );
                self.defined.insert(bind.clone());
                self.emit(quote! { let #bind = dateg::Entry::Var(#rb.#action(#f, #args)); })
            }
            _ => self.emit(quote! { #rb.#action(#f, #args, #bind); }),
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
        Ok(match arg {
            SExpr::Implicit(span) => {
                let tmp = self.new_ident(*span);
                self.maybe_init_var(&tmp, kind)?;
                quote! { #tmp }
            }
            SExpr::Leaf(ident) => {
                self.maybe_init_var(ident, kind)?;
                quote! { #ident }
            }
            SExpr::Value {
                val,
                primitive,
                add_into,
            } => {
                let mut expr = quote! { #val };
                if *primitive {
                    let eg = self.eg;
                    let pre = self.new_ident(expr.span());
                    let into = add_into.then(|| quote! { .into() });
                    self.emit_pre(quote! { let #pre = #eg.add_primitive_value(#expr #into); });
                    expr = quote! { #pre };
                }
                quote! { dateg::Entry::Const(#expr) }
            }
            SExpr::Nested(f, args) => {
                let tmp = self.new_ident(f.span());
                self.emit_atom(kind, &tmp, f, args)?;
                quote! { #tmp }
            }
        })
    }

    fn maybe_init_var(&mut self, var: &Ident, kind: Kind) -> Result<()> {
        if self.defined.insert(var.clone()) {
            if !matches!(kind, Kind::Query) {
                return err!(var.span(), "var was not defined");
            }
            let rb = self.rb.clone();
            self.emit(quote! { let #var = dateg::Entry::Var(#rb.var_named(stringify!(#var))); });
        }
        Ok(())
    }
    fn new_ident(&mut self, span: Span) -> Ident {
        self.counter += 1;
        Ident::new(&format!("__tmp{}", self.counter), span)
    }

    fn unit(&self) -> TokenStream {
        quote::quote_spanned! { self.action_span => dateg::Entry::Const(dateg::token_unit()) }
    }
}

macro_rules! enum_str {
    (enum $Enum:ident { $( $Variant:ident $from:literal $to:literal,)* }) => {
        #[derive(Clone, Copy)]
        enum $Enum { $($Variant,)* }
        impl $Enum {
            fn from_str(s: &str) -> Option<Self> {
                match s {
                    $( $from => Some(Self::$Variant), )*
                    _ => None,
                }
            }
            fn to_str(&self) -> &'static str {
                match self { $( Self::$Variant => $to, )* }
            }
        }
    };
}

enum_str! {
    enum Kind {
        Query "query" "query",
        Add "add" "add",
        Set "set" "set",
    }
}
impl Kind {
    fn args_handling(self) -> Self {
        match self {
            Self::Query => self,
            _ => Self::Add,
        }
    }
}
enum_str! {
    enum KindUnary {
        Contains "contains" "query",
        Insert "insert" "set",
        Call "call" "call",
    }
}
impl KindUnary {
    fn args_handling(self) -> Kind {
        match self {
            Self::Contains => Kind::Query,
            Self::Insert | Self::Call => Kind::Add,
        }
    }
}

impl SExpr {
    fn as_app(&self) -> Result<(&Ident, &[Self])> {
        let span = match self {
            Self::Implicit(span) => *span,
            Self::Leaf(ident) => ident.span(),
            Self::Value { val, .. } => val.span(),
            Self::Nested(f, args) => return Ok((f, args)),
        };
        err!(span, "expected app")
    }
}
