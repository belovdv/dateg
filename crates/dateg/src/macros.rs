/// This macro has no complex logic, just basic syntax sugar (uses [`rule`] macro)
#[macro_export]
#[rust_analyzer::macro_style(braces)]
macro_rules! execute {
    ($eg:expr; $( ($action:tt $($prog:tt)*) )*) => {
        $( $crate::helper!(@highlight_ty $action); )*
        $( $crate::execute!(@ $eg; $action $($prog)*); )*
    };

    // Table
    (@$eg:expr; constructor $table:ident ($($Args:ident)*) $Ret:ident) => {
        $crate::execute!(@@$eg; table new_table_constructor; $table ($($Args)*) $Ret);
    };
    (@$eg:expr; function $table:ident ($($Args:ident)*) $Ret:ident) => {
        $crate::execute!(@@$eg; table new_table_function; $table ($($Args)*) $Ret);
    };
    (@$eg:expr; relation $table:ident ($($Args:ident)*)) => {
        $crate::execute!(@@$eg; table new_table_relation; $table ($($Args)*));
    };
    (@@$eg:expr; table $method:ident; $table:ident ($($Args:ident)*) $($Ret:ident)?) => {
        #[cfg(false)] fn $table() {};
        let $table = $eg.$method::<
            ($($crate::helper!(@token $Args),)*)
            $(, $crate::helper!(@token $Ret))?
        >(stringify!($table));
    };

    // Values
    (@$eg:expr; val $name:ident ($T:ident) {$val:expr}) => {
        let $name = $eg.add_primitive_value::<$T>($val);
    };
    (@$eg:expr; add $name:ident ($table:ident $($args:ident)*)) => {
        #[cfg(false)] fn $table() {}
        let $name = $eg.row_add($table, ($($args,)*));
    };
    (@$eg:expr; set ($table:ident $($args:ident)*) $val:ident) => {
        #[cfg(false)] fn $table() {}
        $eg.row_set($table, ($($args,)*), $val);
    };
    // Helper for relation
    (@$eg:expr; insert ($table:ident $($args:ident)*)) => {
        #[cfg(false)] fn $table() {}
        $eg.row_set($table, ($($args,)*), $crate::token_unit());
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
}

#[macro_export]
#[rust_analyzer::macro_style(parenthesized)]
macro_rules! theory {
    ($Theory:ident
        ($(($sort_kind:ident $Sort:ident))*)
        ($(($action:tt $name:tt $($prog:tt)*))*)
        ($(($action_extra:tt $($prog_extra:tt)*))*)
    ) => {
        $( $crate::helper!(@highlight_ty $action); )*
        $( $crate::helper!(@highlight_ty $action_extra); )*

        impl Default for $Theory {
            fn default() -> Self {
                let mut eg = $crate::EGraph::default();
                $crate::theory!(@sort_init ty eg; ());
                $( $crate::theory!(@sort_init $sort_kind eg; $Sort); )*
                $( $crate::execute!(@eg; $action $name $($prog)*); )*
                $( $crate::execute!(@eg; $action_extra $($prog_extra)*); )*
                Self { eg, $($name),* }
            }
        }

        $( $crate::theory!(@sort_define $sort_kind $Sort); )*
        pub struct $Theory {
            pub eg: $crate::EGraph,
            $( pub $name: $crate::theory!(@field_ty $action $($prog)*), )*
        }

        impl std::ops::Deref for $Theory {
            type Target = $crate::EGraph;
            fn deref(&self) -> &Self::Target {
                &self.eg
            }
        }
        impl std::ops::DerefMut for $Theory {
            fn deref_mut(&mut self) -> &mut Self::Target {
                &mut self.eg
            }
        }
    };

    // Initialize sorts
    (@sort_define sort $Sort:ident) => {
        pub struct $Sort;
        impl $crate::EGraphValue for $Sort {
            type Token = $crate::TokenOpaque<Self>;
        }
    };
    (@sort_define ty $Sort:ident) => {};
    (@sort_init sort $eg:expr; $Sort:ident) => {};
    (@sort_init ty $eg:expr; $Sort:ty) => {
        $eg.add_primitive_type::<$Sort>();
    };

    // Field type dispatch
    (@field_ty constructor ($($Args:ident)*) $Ret:ident) => {
        $crate::Table<$crate::helper!(@triple ($($Args)*) ($Ret) True)>
    };
    (@field_ty function ($($Args:ident)*) $Ret:ident) => {
        $crate::Table<$crate::helper!(@triple ($($Args)*) ($Ret) False)>
    };
    (@field_ty relation ($($Args:ident)*)) => {
        $crate::Table<(
            ($(<$Args as $crate::EGraphValue>::Token,)*),
            dateg::TokenPrimitive<()>,
            $crate::False,
        )>
    };
    (@field_ty val ($Ty:ident) $val:tt) => {
        $crate::helper!(@token $Ty)
    };
}

#[macro_export]
macro_rules! helper {
    // Getting Token type
    (@token $EGV:ident) => { <$EGV as $crate::EGraphValue>::Token };
    (@triple ($($Args:ident)*) ($Ret:ty) $B:ident) => {
        (
            ($(<$Args as $crate::EGraphValue>::Token,)*),
            <$Ret as $crate::EGraphValue>::Token,
            $crate::$B,
        )
    };

    // syntax highlighting hack
    (@highlight_ty $action:tt) => { #[cfg(false)] struct $action {} };
    // For whatever reason this does not work
    (@highlight_fn $action:tt) => { #[cfg(false)] fn $action() {} };
}
