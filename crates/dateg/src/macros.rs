/// This macro has no complex logic, just basic syntax sugar (uses [`rule`] macro)
#[macro_export]
#[rust_analyzer::macro_style(braces)]
macro_rules! execute {
    ($eg:expr; $( ($action:tt $($prog:tt)*) )*) => {
        $( $crate::execute!(@@ $action); )*
        $( $crate::execute!(@ $eg; $action $($prog)*); )*
    };

    // Table
    (@$eg:expr; constructor $table:ident ($($Args:ident)*) $Ret:ident) => {
        #[cfg(false)] fn $table() {} // syntax highlighting hack
        let $table = $eg.new_table_constructor::<($($Args,)*), $Ret>(stringify!($table));
    };
    (@$eg:expr; function $table:ident ($($Args:ident)*) $Ret:ident) => {
        #[cfg(false)] fn $table() {} // syntax highlighting hack
        let $table = $eg.new_table_function::<($($Args,)*), $Ret>(stringify!($table));
    };
    (@$eg:expr; relation $table:ident ($($Args:ident)*)) => {
        #[cfg(false)] fn $table() {} // syntax highlighting hack
        let $table = $eg.new_table_relation::<($($Args,)*)>(stringify!($table));
    };
    (@$eg:expr; get_constructor $table:ident ($($Args:ident)*) $Ret:ident) => {
        #[cfg(false)] fn $table() {} // syntax highlighting hack
        let $table = $eg.get_table::<(($($Args,)*), $Ret, $crate::True)>(stringify!($table));
    };
    (@$eg:expr; get_function $table:ident ($($Args:ident)*) $Ret:ident) => {
        #[cfg(false)] fn $table() {} // syntax highlighting hack
        let $table = $eg.get_table::<($($Args,)*), $Ret>(stringify!($table));
    };
    (@$eg:expr; get_relation $table:ident ($($Args:ident)*)) => {
        #[cfg(false)] fn $table() {} // syntax highlighting hack
        let $table = $eg.get_table::<($($Args,)*)>(stringify!($table));
    };

    // Ruleset
    (@$eg:expr; set_ruleset_active $name:literal) => {
        $eg.ruleset_active = $name.to_string();
    };
    (@$eg:expr; run_ruleset_active) => {
        $eg.run_ruleset_active();
    };
    (@$eg:expr; run_ruleset $name:literal) => {
        $eg.run_ruleset($name);
    };
    // Rule
    (@$eg:expr; rule $($body:tt)*) => {
        $crate::rule!{$eg; $($body)* };
    };
    (@$eg:expr; rewrite ($($lhs:tt)*) ($($rhs:tt)*) $(if $((query $($cond:tt)*))+)?) => {
        $crate::rule!{$eg; (query __r ($($lhs)*)) (set __r ($($rhs)*)) $($((query $($cond)*))+)? };
    };
    (@$eg:expr; birewrite ($($lhs:tt)*) ($($rhs:tt)*) $(if $((query $($cond:tt)*))+)?) => {
        $crate::rule!{$eg; (query __r ($($lhs)*)) (set __r ($($rhs)*)) $($((query $($cond)*))+)? };
        $crate::rule!{$eg; (query __r ($($rhs)*)) (set __r ($($lhs)*)) $($((query $($cond)*))+)? };
    };
    (@$eg:expr; rewrite ($($lhs:tt)*) $rhs:tt) => {
        $crate::rule!{$eg; (query __r ($($lhs)*)) (uni __r $rhs) };
    };

    // Constants
    (@$eg:expr; = $value:ident ($table_token:ident $($args:ident)*)) => {
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
