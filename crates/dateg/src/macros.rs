/// Simple DSL
///
/// Syntax: `execute! {<egraph ref expr>; <action>* }`
///
/// Independently translates actions to method calls on egraph, binds results to local variables
///
/// Adds some hacks for syntax highlighting
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
    (@$eg:expr; function $table:ident ($($Args:ident)*) $Ret:ident :merge $m:ident) => {
        $crate::execute!(@@$eg; table new_table_function_with_merge; $table ($($Args)*) $Ret ($m));
    };
    (@@$eg:expr; table $m:ident; $table:ident ($($Args:ident)*) $($Ret:ident)? $(($arg:tt))?) => {
        #[cfg(false)] fn $table() {};
        let $table = $eg.$m::<
            ($($crate::helper!(@token $Args),)*)
            $(, $crate::helper!(@token $Ret))?
        >(stringify!($table) $(, $arg)?);
    };
    // Evaluation
    (@$eg:expr; evaluation $func:ident ($($Args:ident)*) $Ret:tt {$eval:expr}) => {
        #[cfg(false)] fn $func() {};
        let $func = $eg.new_function::<
            ($($crate::helper!(@token $Args),)*),
            $crate::helper!(@token $Ret)
        >($eval);
    };
    (@$eg:expr; evaluation_partial $func:ident ($($Args:ident)*) $Ret:tt {$eval:expr}) => {
        #[cfg(false)] fn $func() {};
        let $func = $eg.new_function_partial::<
            ($($crate::helper!(@token $Args),)*),
            $crate::helper!(@token $Ret)
        >($eval);
    };

    // Values
    (@$eg:expr; val $name:ident ($T:ident) {$val:expr}) => {
        let $name = $eg.add_primitive_value::<$T>($val);
    };
    (@$eg:expr; add $name:ident ($table:ident $($args:ident)*)) => {
        #[cfg(false)] fn $table() {}
        let $name = $eg.row_add($table, ($($args,)*));
    };
    (@$eg:expr; set $val:ident ($table:ident $($args:ident)*) ) => {
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
    (@$eg:expr; rewrite ($($lhs:tt)*) ($($rhs:tt)*) $(if $(($($cond:tt)*))+)?) => {
        $crate::rule!{$eg; (query __r ($($lhs)*)) (set __r ($($rhs)*)) $($(($($cond)*))+)? };
    };
    (@$eg:expr; birewrite ($($lhs:tt)*) ($($rhs:tt)*) $(if $(($($cond:tt)*))+)?) => {
        $crate::rule!{$eg; (query __r ($($lhs)*)) (set __r ($($rhs)*)) $($(($($cond)*))+)? };
        $crate::rule!{$eg; (query __r ($($rhs)*)) (set __r ($($lhs)*)) $($(($($cond)*))+)? };
    };
    (@$eg:expr; rewrite ($($lhs:tt)*) $rhs:tt) => {
        $crate::rule!{$eg; (query __r ($($lhs)*)) (uni __r $rhs) };
    };
}

/// Defines `Theory` structure with `Default` implementation, initializing `EGraph`
///
/// Built on top of [`execute`]
///
/// Primary motivation - wrap everything produced by [`execute`] into structure for later usage
///
/// Additionally does some initialization for types (primitive and opaque)
///
/// Syntax:
/// ```no_run
/// # macro_rules! theory { ($($tt:tt)*) => { () }; };
/// theory!(TheoryName(
///     (sort <opaque sort name>)  // adds unit type and implements `EGraphValue` for it
///     (ty <primitive sort name>) // adds to initialization of `EGraph`
/// )(
///     <action>* // These actions should produce results, they will be added as fields to `Theory`
/// )(
///     <action>* // These actions will be added to `default` as `execute! {self; <actions>* }`
/// )
/// <{<self_>; arbitrary post-init code }>?
/// );
/// ```
#[macro_export]
#[rust_analyzer::macro_style(parenthesized)]
macro_rules! theory {
    ($Theory:ident
        ($(($sort_kind:ident $Sort:ident))*)
        ($(($action:tt $name:tt $($prog:tt)*))*)
        ($(($action_extra:tt $($prog_extra:tt)*))*)
        $({$($post_init:tt)*})?
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
                eg.set_ruleset_active("");
                #[allow(unused_mut)]
                let mut r = Self { eg, $($name),* };
                $( let $self_ = &mut r; {$($post_init)*} )?
                r
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
    (@field_ty function ($($Args:ident)*) $Ret:ident $(:merge $merge:ident)?) => {
        $crate::Table<$crate::helper!(@triple ($($Args)*) ($Ret) False)>
    };
    (@field_ty relation ($($Args:ident)*)) => {
        $crate::Table<(
            ($(<$Args as $crate::EGraphValue>::Token,)*),
            dateg::TokenPrimitive<()>,
            $crate::False,
        )>
    };
    (@field_ty evaluation ($($Args:ident)*) $Ret:tt {$($tt:tt)*}) => {
        $crate::Function<$crate::helper!(@triple ($($Args)*) ($Ret) True)>
    };
    (@field_ty evaluation_partial ($($Args:ident)*) $Ret:tt {$($tt:tt)*}) => {
        $crate::Function<$crate::helper!(@triple ($($Args)*) ($Ret) True)>
    };
    (@field_ty val ($Ty:ident) $val:tt) => {
        $crate::helper!(@token $Ty)
    };
}

#[macro_export]
macro_rules! helper {
    // Getting Token type
    (@token $EGV:tt) => { <$EGV as $crate::EGraphValue>::Token };
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
