pub trait TupleScannerInput<Mapper> {
    type Output;
    fn proc(self, mapper: &mut Mapper) -> Self::Output;
}
pub trait TupleScan<Mapper> {
    type Output;
    fn scan(self, mapper: &mut Mapper) -> Self::Output;
}

macro_rules! impl_ts {
    (@ $(($From:ident $To:ident))*) => {
        impl<Mapper, $($From),* $(,$To)*> TupleScan<Mapper> for ($($From,)*)
        where
            $( $From: TupleScannerInput<Mapper, Output = $To>, )*
        {
            type Output = ($($To,)*);
            fn scan(self, mapper: &mut Mapper) -> ($($To,)*) {
                let _ = mapper;
                #[allow(non_snake_case)]
                let ($($From,)*) = self;
                ($($From.proc(mapper),)*)
            }
        }
    };
    () => {
        impl_ts!(@);
    };
    ($head:tt $($tail:tt)*) => {
        impl_ts!(@ $head $($tail)*);
        impl_ts!($($tail)*);
    };
}
impl_ts!(
    (A1 A2) (B1 B2) (C1 C2) (D1 D2) (E1 E2)
    (F1 F2) (G1 G2) (H1 H2) (I1 I2) (J1 J2)
);

#[macro_export]
macro_rules! tuple_scanner {
    ($Scanner:ident$(($($Args:ty),*))?; $(
        fn $(<$Ty:ident $(: $Bound:path)?>)?
            ($s:tt: $In:ty, $mapper:tt) -> $Out:ty { $($body:tt)* }
    )*) => {
        pub struct $Scanner$(($(pub $Args),*))?;
        $( impl $(<$Ty $(: $Bound)?>)? $crate::TupleScannerInput<$Scanner> for $In {
            type Output = $Out;
            fn proc(self, $mapper: &mut $Scanner) -> Self::Output {
                let $s = self; $($body)*
            }
        } )*
        impl $Scanner {
            pub fn scan<Tuple, Output>(&mut self, tuple: Tuple) -> Output
            where Tuple: $crate::TupleScan<Self, Output = Output>, {
                $crate::TupleScan::scan(tuple, self)
            }
        }
    };
    // TODO: support generics. Procedural macros are likely needed.
    ($Scanner:ident<$lt:lifetime>$(($($Args:ty),*))?; $(
        fn $(<$Ty:ident $(: $Bound:path)?>)?
            ($s:tt: $In:ty, $mapper:tt) -> $Out:ty { $($body:tt)* }
    )*) => {
        pub struct $Scanner<$lt>$(($(pub $Args),*))?;
        $( impl $(<$Ty $(: $Bound)?>)? $crate::TupleScannerInput<$Scanner<'_>> for $In {
            type Output = $Out;
            fn proc(self, $mapper: &mut $Scanner) -> Self::Output {
                let $s = self; $($body)*
            }
        } )*
        impl<$lt> $Scanner<$lt> {
            pub fn scan<Tuple, Output>(&mut self, tuple: Tuple) -> Output
            where Tuple: $crate::TupleScan<Self, Output = Output>, {
                $crate::TupleScan::scan(tuple, self)
            }
        }
    };
}

pub trait TupleIntoArray<const N: usize, T> {
    type Array;
    fn into_array(self) -> [T; N];
}

macro_rules! impl_ia {
    (@ $TT:ident $N:literal ($($T:ident)*) ($($x:ident)*)) => {
        impl<$TT> TupleIntoArray<$N, T> for ($($T,)*) {
            type Array = [$TT; $N];
            fn into_array(self) -> [$TT; $N] {
                let ($($x,)*) = self;
                [$($x),*]
            }
        }
    };
    (@@ $TT:ident () () ()) => {
        impl_ia!(@ $TT 0 () ());
    };
    (@@ $TT:ident ($N:tt $($N_:tt)*) ($T:ident $($T_:ident)*) ($x:ident $($x_:ident)*)) => {
        impl_ia!(@ $TT $N ($T $($T_)*) ($x $($x_)*));
        impl_ia!(@@ $TT ($($N_)*) ($($T_)*) ($($x_)*));
    };
}
impl_ia!(@@ T (10 9 8 7 6 5 4 3 2 1) (T T T T T T T T T T) (a b c d e f g h i j));

#[test]
fn tuple_map() {
    struct S1;
    struct S2;
    let t = (0usize, "", S1, 0.1f64, 2usize, 3usize, S1);
    tuple_scanner!(Mapper1;
        fn (s: usize, _) -> isize { s as isize }
        fn (s: &str, _) -> String { s.to_string() }
        fn (_: S1, _) -> S2 { S2 }
        fn (s: f64, _) -> String { format!("{s}") }
    );
    let r = Mapper1.scan(t);
    let _: String = r.3;

    tuple_scanner!(Mapper2(usize);
        fn (s: usize, m) -> usize { s + m.0 }
        fn (s: &str, m) -> String { format!("{s}{}", m.0) }
    );
    let r = Mapper2(1).scan((2, "3"));
    assert_eq!(r, (3, "31".to_string()));

    tuple_scanner!(SumUsize(usize);
        fn (s: usize, p) -> () { p.0 += s; }
        fn (_: &str, _) -> () {}
    );
    let mut sum = SumUsize(0);
    sum.scan((0, 1, 2, 3, "", "", 6));
    assert_eq!(sum.0, 12);

    tuple_scanner!(MapDisplay;
        fn <T: std::fmt::Display>(t: T, _) -> String { format!("{t}") }
    );
    let r = MapDisplay.scan((0, 1, "a", true, 1.1));
    assert_eq!(r.0, "0");
    assert_eq!(r.1, "1");
    assert_eq!(r.2, "a");
    assert_eq!(r.3, "true");
    assert_eq!(r.4, "1.1");

    // Also test [`TupleIntoArray`]
    let r: [String; _] = r.into_array();
    let r: Vec<_> = r.iter().collect();
    assert_eq!(&r, &["0", "1", "a", "true", "1.1"]);
}
