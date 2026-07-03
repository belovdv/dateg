/// This macro has no complex logic, just basic syntax sugar
#[macro_export]
#[rust_analyzer::macro_style(braces)]
macro_rules! execute {
    ($eg:expr; $( ($($prog:tt)*) )*) => {
        $( crate::execute!(@ $eg; $($prog)*); )*
    };
    (@ $eg:expr; $table:ident = constructor $Table:ident ($($Args:ident)*) $Ret:ident) => {
        #[cfg(false)] fn $table() {} // syntax highlighting hack
        let $table = $eg.add_table_constructor::<($($Args,)*), $Ret>(stringify!($Table));
    };
    (@ $eg:expr; $table:ident = function $Table:ident ($($Args:ident)*) $Ret:ident) => {
        #[cfg(false)] fn $table() {} // syntax highlighting hack
        let $table = $eg.add_table_function::<($($Args,)*), $Ret>(stringify!($Table));
    };
    (@ $eg:expr; $value:ident = ($table_token:ident $($args:ident)*)) => {
        #[cfg(false)] fn $table_token() {} // syntax highlighting hack
        let $value = $eg.row_add($table_token, ($($args,)*));
    };
}
