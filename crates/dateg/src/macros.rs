/// This macro has no complex logic, just basic syntax sugar
#[macro_export]
#[rust_analyzer::macro_style(braces)]
macro_rules! execute {
    ($eg:expr; $( ($($prog:tt)*) )*) => {
        $( crate::execute!(@ $eg; $($prog)*); )*
    };
    (@ $eg:expr; constructor $table:ident ($($Args:ident)*) $Ret:ident) => {
        #[cfg(false)] fn $table() {} // syntax highlighting hack
        let $table = $eg.add_table_constructor::<($($Args,)*), $Ret>(stringify!($table));
    };
    (@ $eg:expr; function $table:ident ($($Args:ident)*) $Ret:ident) => {
        #[cfg(false)] fn $table() {} // syntax highlighting hack
        let $table = $eg.add_table_function::<($($Args,)*), $Ret>(stringify!($table));
    };
    (@ $eg:expr; $value:ident = ($table_token:ident $($args:ident)*)) => {
        #[cfg(false)] fn $table_token() {} // syntax highlighting hack
        let $value = $eg.row_add($table_token, ($($args,)*));
    };
}


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
        $( crate::rule!(@$action rb $($args)*); )*
        rb.build()
    }};
    (@query $rb:ident $bind:ident ($table:ident $($args:ident)*)) => {
        #[cfg(false)] fn $table() {} // syntax highlighting hack
        $rb.query($table, ($($args,)*), $bind);
    };
    (@add $rb:ident $bind:ident ($table:ident $($args:ident)*)) => {
        #[cfg(false)] fn $table() {} // syntax highlighting hack
        let $bind = $rb.add($table, ($($args,)*));
    };
    (@set $rb:ident $bind:ident ($table:ident $($args:ident)*)) => {
        #[cfg(false)] fn $table() {} // syntax highlighting hack
        $rb.set($table, ($($args,)*), $bind);
    };
    (@uni $rb:ident $a:ident $b:ident) => {
        $rb.union($a, $b);
    };
}