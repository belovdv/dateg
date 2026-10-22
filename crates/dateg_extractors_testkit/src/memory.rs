use dateg::{ContainerVec, TokenContainer, TokenOpaque, TokenPrimitive, theory};

pub type Ints = ContainerVec<Int>;

theory!(Memory(
    (sort Mem)
    (sort Int)
    (sort IntTuple)
    (container Ints)
    (ty String)
    (ty usize)
)(
    (constructor init () Mem)
    // Constructor roles (cost and `consumes`) are not declared here: the
    // extractor index does that. In `indexes/mem.rs`, `store` consumes the
    // memory token and `load` does not.
    (constructor store (Mem Int Int) Mem)
    (constructor load (Mem Int) Int)

    (constructor var (String) Int)
    (constructor cst (usize) Int)
    (constructor one () Int)
    (constructor inc (Int) Int)
    (constructor add (Int Int) Int)
    (constructor sub (Int Int) Int)
    (constructor neg (Int) Int)
    (constructor mul (Int Int) Int)
    (constructor square (Int) Int)
    (constructor dec (Int) Int)

    (constructor expr (String IntTuple) Int)
    (constructor expr0 () IntTuple)
    (constructor expr1 (Int) IntTuple)
    (constructor expr2 (Int Int) IntTuple)
    (constructor expr_v (String Ints) Int)

    (evaluation expr0_v () Ints { |()| ContainerVec(vec![]) })
    (evaluation expr1_v (Int) Ints { |(a,)| ContainerVec(vec![a]) })
    (evaluation expr2_v (Int Int) Ints { |(a, b)| ContainerVec(vec![a, b]) })
    (evaluation_partial usize_neq (usize usize) () { |(a, b)| if a != b { Some(()) } else { None } })
    (relation neq (Int Int))
    (relation is_comas (String))

    (val zero_ (usize) {0})
    (val s_one (String) {"one".into()})
    (val s_mul (String) {"mul".into()})
    (val s_add (String) {"add".into()})
)(
    (add _init (init))
    (add zero (cst zero_))
    (insert (is_comas s_mul))
    (insert (is_comas s_add))

    (set_ruleset_active "tree")
    (birewrite (one) (expr "one" (expr0)))
    (birewrite (inc x) (expr "inc" (expr1 x)))
    (birewrite (square x) (expr "square" (expr1 x)))
    (birewrite (mul x y) (expr "mul" (expr2 x y)))
    (birewrite (add x y) (expr "add" (expr2 x y)))
    (rewrite (expr s (expr0)) (expr_v s (expr0_v)))
    (rewrite (expr s (expr1 a)) (expr_v s (expr1_v a)))
    (rewrite (expr s (expr2 a b)) (expr_v s (expr2_v a b)))
    (rewrite
        (expr name (expr2 x y))
        (expr name (expr2 y x))
        if (contains (is_comas name))
    )
    (birewrite
        (expr name (expr2 x (expr name (expr2 y z))))
        (expr name (expr2 (expr name (expr2 x y)) z))
        if (contains (is_comas name))
    )
    (birewrite (inc x) (add x (one)))
    (rule
        (query r (expr name args))
        (set r (mul r (one)))
    )
    (birewrite (add (mul x y) y) (mul (inc x) y))
    (birewrite (mul x x) (square x))

    (set_ruleset_active "square_mul_l2r")
    (rewrite (mul x x) (square x))

    (set_ruleset_active "square_mul_r2l")
    (rewrite (square x) (mul x x))

    (set_ruleset_active "mul_add")
    (birewrite (inc x) (add x (one)))
    (birewrite (add (mul x y) y) (mul (inc x) y))

    (set_ruleset_active "read_after_write")
    (rewrite (load (store mem addr val) addr) val)
    (rewrite (store mem addr (load mem addr)) mem)

    (set_ruleset_active "store_reorder")
    (rewrite
        (store (store mem addr1 val1) addr2 val2)
        (store (store mem addr2 val2) addr1 val1)
        if (contains (neq addr1 addr2))
    )
    (rewrite (store (store mem addr _) addr val) (store mem addr val))
    (rule
        (query a (cst ca))
        (query b (cst cb))
        (query _ (usize_neq ca cb))
        (insert (neq a b))
    )

    (set_ruleset_active "arith")
    (rewrite (add a {zero}) a)
    (rewrite (sub a a) {zero})
    (birewrite (sub a b) (add a (neg b)))
    (rewrite (neg {zero}) {zero})
    (rewrite (neg a) (sub {zero} a))
    (rewrite (neg (neg x)) x)

    (set_ruleset_active "comm")
    (rewrite (add a b) (add b a))
    (rewrite (mul a b) (mul b a))

    // Applied once between saturation passes: this is the bound-1 behaviour.
    (set_ruleset_active "assoc")
    (rewrite (add a (add b c)) (add (add a b) c))
    (rewrite (add (add a b) c) (add a (add b c)))
    (rewrite (mul a (mul b c)) (mul (mul a b) c))
    (rewrite (mul (mul a b) c) (mul a (mul b c)))

    (set_ruleset_active "alias")
    (rewrite (load (store mem addr val) addr) val)
    (rewrite
        (load (store mem addr1 _) addr2)
        (load mem addr2)
        if (contains (neq addr1 addr2))
    )
    (rewrite
        (store (store mem addr1 val1) addr2 val2)
        (store (store mem addr2 val2) addr1 val1)
        if (contains (neq addr1 addr2))
    )
    (rewrite (store (store mem addr _) addr val) (store mem addr val))
    (rewrite (store mem addr (load mem addr)) mem)
    (rule
        (query a (cst ca))
        (query b (cst cb))
        (query _ (usize_neq ca cb))
        (insert (neq a b))
    )
)
{
    m;
    m.set_single_thread();
});

pub fn inc_chain(n: usize) -> (Memory, TokenOpaque<Int>) {
    let mut m = Memory::default();
    let mut value = m.add_one();
    for _ in 1..n {
        value = m.add_inc(value);
    }
    let value = value.canon(&m.eg);
    (m, value)
}

pub fn saturate(m: &mut Memory, rulesets: &[&str]) {
    for _ in 0..100 {
        let mut changed = false;
        for ruleset in rulesets {
            changed |= m.run_ruleset(ruleset);
        }
        if !changed {
            return;
        }
    }
    panic!("rulesets did not converge: {rulesets:?}");
}

/// Run the normal rulesets to saturation, apply associativity once, then
/// saturate again (bounded associativity, without cost tracking).
pub fn saturate_with_assoc(m: &mut Memory, rulesets: &[&str]) {
    saturate(m, rulesets);
    m.run_ruleset("assoc");
    saturate(m, rulesets);
}

impl Memory {
    pub fn add_init(&mut self) -> TokenOpaque<Mem> {
        let table = self.init;
        self.row_add(table, ())
    }

    pub fn add_cst(&mut self, value: usize) -> TokenOpaque<Int> {
        let value = self.add_primitive_value(value);
        let table = self.cst;
        self.row_add(table, (value,))
    }

    pub fn add_one(&mut self) -> TokenOpaque<Int> {
        let table = self.one;
        self.row_add(table, ())
    }

    pub fn add_inc(&mut self, value: TokenOpaque<Int>) -> TokenOpaque<Int> {
        let table = self.inc;
        self.row_add(table, (value,))
    }

    pub fn add_mul(&mut self, a: TokenOpaque<Int>, b: TokenOpaque<Int>) -> TokenOpaque<Int> {
        let table = self.mul;
        self.row_add(table, (a, b))
    }

    pub fn add_add(&mut self, a: TokenOpaque<Int>, b: TokenOpaque<Int>) -> TokenOpaque<Int> {
        let table = self.add;
        self.row_add(table, (a, b))
    }

    pub fn add_sub(&mut self, a: TokenOpaque<Int>, b: TokenOpaque<Int>) -> TokenOpaque<Int> {
        let table = self.sub;
        self.row_add(table, (a, b))
    }

    pub fn add_square(&mut self, value: TokenOpaque<Int>) -> TokenOpaque<Int> {
        let table = self.square;
        self.row_add(table, (value,))
    }

    pub fn add_neg(&mut self, value: TokenOpaque<Int>) -> TokenOpaque<Int> {
        let table = self.neg;
        self.row_add(table, (value,))
    }

    pub fn add_store(
        &mut self,
        mem: TokenOpaque<Mem>,
        addr: TokenOpaque<Int>,
        value: TokenOpaque<Int>,
    ) -> TokenOpaque<Mem> {
        let table = self.store;
        self.row_add(table, (mem, addr, value))
    }

    pub fn add_load(&mut self, mem: TokenOpaque<Mem>, addr: TokenOpaque<Int>) -> TokenOpaque<Int> {
        let table = self.load;
        self.row_add(table, (mem, addr))
    }

    pub fn add_expr_v(
        &mut self,
        name: TokenPrimitive<String>,
        args: TokenContainer<Ints>,
    ) -> TokenOpaque<Int> {
        let table = self.expr_v;
        self.row_add(table, (name, args))
    }

    pub fn get_val(&self, value: usize) -> TokenOpaque<Int> {
        let mut expr = self.eg.row_get(self.one, ()).unwrap();
        for v in 1..value {
            let err = || panic!("{v}");
            expr = self.eg.row_get(self.inc, (expr,)).unwrap_or_else(err);
        }
        expr.canon(&self.eg)
    }

    /// Number of constructor rows currently present, used as a simple test
    /// complexity estimate. Rules can add rows; call after saturation.
    pub fn term_count(&self) -> usize {
        let mut count = 0;
        macro_rules! tables {
            ($($table:ident),* $(,)?) => {
                $( self.eg.for_each_row(self.$table, |_, _| count += 1); )*
            };
        }
        tables!(
            init, store, load, var, cst, one, inc, add, sub, neg, mul, square, dec, expr, expr0,
            expr1, expr2, expr_v,
        );
        count
    }
}
