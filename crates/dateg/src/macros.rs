/// This macro has no complex logic, just basic syntax sugar (uses [`rule`] macro)
#[macro_export]
#[rust_analyzer::macro_style(braces)]
macro_rules! execute {
    ($eg:expr; $( ($action:tt $($prog:tt)*) )*) => {
        $( $crate::execute!(@@ $action); )*
        $( $crate::execute!(@ $eg; $action $($prog)*); )*
    };
    (@ $eg:expr; constructor $table:ident ($($Args:ident)*) $Ret:ident) => {
        #[cfg(false)] fn $table() {} // syntax highlighting hack
        let $table = $eg.add_table_constructor::<($($Args,)*), $Ret>(stringify!($table));
    };
    (@ $eg:expr; function $table:ident ($($Args:ident)*) $Ret:ident) => {
        #[cfg(false)] fn $table() {} // syntax highlighting hack
        let $table = $eg.add_table_function::<($($Args,)*), $Ret>(stringify!($table));
    };
    (@ $eg:expr; = $value:ident ($table_token:ident $($args:ident)*)) => {
        #[cfg(false)] fn $table_token() {} // syntax highlighting hack
        let $value = $eg.row_add($table_token, ($($args,)*));
    };
    (@ $eg:expr; rule $rule:ident $($body:tt)*) => {
        let $rule = $crate::rule!{$eg; $($body)* };
    };
    (@@ =) => {};
    (@@ $action:ident) => {
        #[cfg(false)] struct $action {} // syntax highlighting hack
    };
}

/// Macro to simplify writing rules
#[macro_export]
#[rust_analyzer::macro_style(braces)]
macro_rules! rule {
    (
        $eg:expr; { $($var:tt)* }
        $( ( $action:ident $($args:tt)* ) )*
    ) => {{
        $( #[cfg(false)] struct $action; )* // syntax highlighting hack
        let mut rb = $eg.rule_builder();
        $( let $var = rb.var_named(stringify!($var)); )*
        $( $crate::rule!(@$action rb $($args)*); )*
        rb.build()
    }};

    (@query $rb:ident $bind:ident ($table:ident $($args:tt)*)) => {
        #[cfg(false)] fn $table() {} // syntax highlighting hack
        let args = ($($crate::rule!(@query_arg $rb $args),)*);
        $rb.query($table, args, $bind);
    };
    // Support for nested syntax
    (@query_arg $rb:ident $arg:ident) => { $arg };
    (@query_arg $rb:ident ($table:ident $($args:tt)*)) => {{
        let arg = $rb.var();
        $crate::rule!(@query $rb arg ($table $($args)*));
        arg
    }};

    (@add $rb:ident $bind:ident ($table:ident $($args:tt)*)) => {
        #[cfg(false)] fn $table() {} // syntax highlighting hack
        let args = ($($crate::rule!(@add_arg $rb $args),)*);
        let $bind = $rb.add($table, args);
    };
    // Support for nested syntax
    (@add_arg $rb:ident $arg:ident) => { $arg };
    (@add_arg $rb:ident ($table:ident $($args:tt)*)) => {{
        $crate::rule!(@add $rb arg ($table $($args)*));
        arg
    }};

    (@set $rb:ident $bind:ident ($table:ident $($args:tt)*)) => {
        #[cfg(false)] fn $table() {} // syntax highlighting hack
        let args = ($($crate::rule!(@add_arg $rb $args),)*);
        $rb.set($table, args, $bind);
    };
    (@uni $rb:ident $a:tt $b:tt) => {
        let a = $crate::rule!(@add_arg $rb $a);
        let b = $crate::rule!(@add_arg $rb $b);
        $rb.union($a, $b);
    };
}
