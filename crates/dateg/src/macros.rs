/// This macro has no complex logic, just basic syntax sugar (uses [`rule`] macro)
#[macro_export]
#[rust_analyzer::macro_style(braces)]
macro_rules! execute {
    ($eg:expr; $( ($action:tt $($prog:tt)*) )*) => {
        $( $crate::execute!(@@ $action); )*
        $( $crate::execute!(@ $eg; $action $($prog)*); )*
    };
    (@$eg:expr; constructor $table:ident ($($Args:ident)*) $Ret:ident) => {
        #[cfg(false)] fn $table() {} // syntax highlighting hack
        let $table = $eg.add_table_constructor::<($($Args,)*), $Ret>(stringify!($table));
    };
    (@$eg:expr; function $table:ident ($($Args:ident)*) $Ret:ident) => {
        #[cfg(false)] fn $table() {} // syntax highlighting hack
        let $table = $eg.add_table_function::<($($Args,)*), $Ret>(stringify!($table));
    };
    (@$eg:expr; relation $table:ident ($($Args:ident)*)) => {
        #[cfg(false)] fn $table() {} // syntax highlighting hack
        let $table = $eg.add_table_relation::<($($Args,)*)>(stringify!($table));
    };
    (@$eg:expr; rule $rule:ident $($body:tt)*) => {
        let $rule = $crate::rule!{$eg; $($body)* };
    };
    (@$eg:expr; run_rules $($rule:ident)*) => {
        $eg.run_rules(&[$($rule),*]);
    };

    (@$eg:expr; = $value:tt ($table_token:ident $($args:ident)*)) => {
        #[cfg(false)] fn $table_token() {} // syntax highlighting hack
        let $value = $eg.row_add($table_token, ($($args,)*));
    };
    // Support for relation
    (@$eg:expr; = () ($table_token:ident $($args:tt)*)) => {
        #[cfg(false)] fn $table_token() {} // syntax highlighting hack
        $eg.row_set($table_token, ($($args,)*), dateg::token_unit());
    };

    (@@ =) => {};
    (@@ $action:ident) => {
        #[cfg(false)] struct $action {} // syntax highlighting hack
    };
}
