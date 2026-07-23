use ahash::{AHashMap, AHashSet};
use dateg::*;
use dateg_extractors::dag::{IndexFor, index_dag};

theory!(Memory(
    (sort Mem)
    (sort Int)
    (sort Pair)
    (ty String)
    (ty usize)
)(
    (constructor init () Mem)
    (constructor load (Mem Int) Pair)
    (constructor load_mem (Pair) Mem)
    (constructor load_val (Pair) Int)
    (constructor store (Mem Int Int) Mem)

    (constructor var (String) Int)
    (constructor cst (usize) Int)
    (constructor add (Int Int) Int)
    (constructor sub (Int Int) Int)
    (constructor neg (Int) Int)

    (relation neq_zero (Int))
    (relation neq (Int Int))
)(
    (add _init (init))

    (val zero_ (usize) {0})
    (add zero (cst zero_))

    (evaluation_partial usize_neq (usize usize) () { |(a, b)| (a != b).then(|| ()) })
    (rule
        (query a (cst ca))
        (query b (cst cb))
        (query _ (usize_neq ca cb))
        (insert (neq a b))
    )

    (rewrite (add a {zero}) a)
    (rewrite (add a b) (add b a))
    (birewrite (add a (add b c)) (add (add a b) c))
    (rewrite (sub a a) {zero})
    (birewrite (sub a b) (add a (neg b)))
    (rewrite (neg {zero}) {zero})
    (rewrite (neg a) (sub {zero} a))

    (evaluation print (usize) () { |(ca,)| { dbg!(ca); } })
    (rule
        (query a (cst ca))
        (query _ (usize_neq ca {zero_}))
        (insert (neq_zero a))
        (call (print ca))
    )
    (rule
        (query r (add a b))
        (contains (neq_zero b))
        (insert (neq a b))
    )
    (rule
        (contains (neq_zero x))
        (insert (neq_zero (neg x)))
    )
    (rewrite (neg (neg x)) x)
    (rule
        (query r1 (add a b))
        (query r2 (add a c))
        (contains (neq b c))
        (insert (neq r1 r2))
    )
    (rule
        (query n (neg a))
        (contains (neq_zero a))
        (insert (neq a n))
    )

    (rewrite (load_mem (load mem _)) mem)
    (rewrite (load_val (load (store mem addr val) addr)) val)
    (rewrite
        (load_val (load (store mem addr1 _) addr2))
        (load_val (load mem addr2))
        if (contains (neq addr1 addr2))
    )
    (rewrite
        (store (store mem addr1 val1) addr2 val2)
        (store (store mem addr2 val2) addr1 val1)
        if (contains (neq addr1 addr2))
    )
    (rewrite (store (store mem addr _) addr val) (store mem addr val))
    (rewrite (store mem addr (load_val (load mem addr))) mem)
));

index_dag!(Index
    mem: EMem (datatype Mem
        Init ()
        Store (Mem Int Int) { |_, _| Some(1) } { |(mem, ..)| (mem,) }
    )
    pair: EPair (datatype Pair
        Load (Mem Int)
    )
    int: EInt (datatype Int
        Cst (usize)
        Var (String)
        LoadVal (Pair)
        Add (Int Int)
        Sub (Int Int)
    )
);

struct IndexToString<'a> {
    m: &'a Memory,
    index: &'a Index,
    out: Vec<String>,
    v: AHashMap<Value, usize>,
}
impl IndexToString<'_> {
    fn eprint(&self) {
        for (n, out) in self.out.iter().enumerate() {
            eprintln!("{n}\t{out}")
        }
    }
    fn push(&mut self, s: impl ToString) -> usize {
        let id = self.out.len();
        self.out.push(s.to_string());
        id
    }
    fn mem(&mut self, t: TokenOpaque<Mem>) -> usize {
        if let Some(id) = self.v.get(&t.into_egglog()) {
            return *id;
        }
        let id = match self.index.value(t) {
            EMem::Init() => self.push("init"),
            EMem::Store(mem, addr, val) => {
                let mem = self.mem(mem);
                let addr = self.int(addr);
                let val = self.int(val);
                self.push(format!("st {mem} {addr} {val}"))
            }
        };
        self.v.insert(t.into_egglog(), id);
        id
    }
    fn pair(&mut self, t: TokenOpaque<Pair>) -> usize {
        if let Some(id) = self.v.get(&t.into_egglog()) {
            return *id;
        }
        let EPair(mem, addr) = self.index.value(t);
        let mem = self.mem(mem);
        let addr = self.int(addr);
        let id = self.push(format!("load {mem} {addr}"));
        self.v.insert(t.into_egglog(), id);
        id
    }
    fn int(&mut self, t: TokenOpaque<Int>) -> usize {
        if let Some(id) = self.v.get(&t.into_egglog()) {
            return *id;
        }
        let id = match self.index.value(t) {
            EInt::Cst(val) => {
                let v = val.get(self.m);
                self.push(format!("cst {v}"))
            }
            EInt::Var(name) => {
                let n = name.get(self.m);
                self.push(format!("var {n}"))
            }
            EInt::LoadVal(pair) => {
                let pair_id = self.pair(pair);
                self.push(format!("load_val {pair_id}"))
            }
            EInt::Add(a, b) => {
                let a = self.int(a);
                let b = self.int(b);
                self.push(format!("add {a} {b}"))
            }
            EInt::Sub(a, b) => {
                let a = self.int(a);
                let b = self.int(b);
                self.push(format!("sub {a} {b}"))
            }
        };
        self.v.insert(t.into_egglog(), id);
        id
    }
}

type Id = usize;
#[derive(Debug, PartialEq, Eq)]
enum Stmt {
    Var(String),
    Const(usize),
    Add(Id, Id),
    Sub(Id, Id),
    LD(Id),
    ST(Id, Id),
}
#[derive(Debug, Default, PartialEq, Eq)]
struct IR(Vec<Stmt>);

impl IR {
    fn push(&mut self, stmt: Stmt) -> Id {
        let id = self.0.len();
        self.0.push(stmt);
        id
    }
    pub fn var(&mut self, name: impl ToString) -> Id {
        self.push(Stmt::Var(name.to_string()))
    }
    pub fn cst(&mut self, val: usize) -> Id {
        self.push(Stmt::Const(val))
    }
    pub fn add(&mut self, a: Id, b: Id) -> Id {
        self.push(Stmt::Add(a, b))
    }
    pub fn sub(&mut self, a: Id, b: Id) -> Id {
        self.push(Stmt::Sub(a, b))
    }
    pub fn ld(&mut self, addr: Id) -> Id {
        self.push(Stmt::LD(addr))
    }
    pub fn st(&mut self, addr: Id, val: Id) {
        self.0.push(Stmt::ST(addr, val));
    }

    fn to_egraph(&self, m: &mut Memory) -> TokenOpaque<Mem> {
        let mut mem = m.row_get(m.init, ()).unwrap();
        let var = m.var;
        let cst = m.cst;
        let add = m.add;
        let sub = m.sub;
        let load = m.load;
        let load_mem = m.load_mem;
        let load_val = m.load_val;
        let store = m.store;
        let mut values: Vec<Option<TokenOpaque<Int>>> = vec![];
        for stmt in self.0.iter() {
            let val = |id: &usize| values[*id].unwrap();
            let value = match stmt {
                Stmt::Var(name) => {
                    let val = m.add_primitive_value(name.to_string());
                    Some(m.row_add(var, (val,)))
                }
                Stmt::Const(val) => {
                    let val = m.add_primitive_value(*val);
                    Some(m.row_add(cst, (val,)))
                }
                Stmt::Add(a, b) => Some(m.row_add(add, (val(a), val(b)))),
                Stmt::Sub(a, b) => Some(m.row_add(sub, (val(a), val(b)))),
                Stmt::LD(addr) => {
                    let pair = m.row_add(load, (mem, val(addr)));
                    mem = m.row_add(load_mem, (pair,));
                    Some(m.row_add(load_val, (pair,)))
                }
                Stmt::ST(addr, v) => {
                    mem = m.row_add(store, (mem, val(addr), val(v)));
                    None
                }
            };
            values.push(value);
        }
        mem
    }

    fn from_egraph(eg: &EGraph, index: Index, root: TokenOpaque<Mem>) -> Self {
        let c = Collector {
            index,
            ..Default::default()
        };
        let mut c_init = CollectorInit {
            inner: c,
            visited: Default::default(),
        };
        c_init.mem(root);
        let mut c = c_init.inner;
        c.mem(root, eg);
        Self(c.ir)
    }
}

#[derive(Default)]
struct Collector {
    ir: Vec<Stmt>,
    index: Index,
    followed_by: AHashMap<TokenOpaque<Mem>, Vec<TokenOpaque<Pair>>>,
    int2id: AHashMap<TokenOpaque<Int>, Id>,
    pair2id: AHashMap<TokenOpaque<Pair>, Id>,
}
struct CollectorInit {
    inner: Collector,
    visited: AHashSet<Value>,
}

impl CollectorInit {
    fn mem(&mut self, mem: TokenOpaque<Mem>) {
        let new = self.visited.insert(mem.into_egglog());
        assert!(new);
        match self.inner.index.value(mem) {
            EMem::Init() => {}
            EMem::Store(mem, addr, val) => {
                self.mem(mem);
                self.int(addr);
                self.int(val);
            }
        }
    }
    fn pair(&mut self, pair: TokenOpaque<Pair>) {
        if !self.visited.insert(pair.into_egglog()) {
            return;
        }
        let EPair(mem, addr) = self.inner.index.value(pair);
        self.int(addr);
        self.inner.followed_by.entry(mem).or_default().push(pair);
    }
    fn int(&mut self, int: TokenOpaque<Int>) {
        if !self.visited.insert(int.into_egglog()) {
            return;
        }
        match self.inner.index.value(int) {
            EInt::Cst(_) => {}
            EInt::Var(_) => {}
            EInt::LoadVal(pair) => self.pair(pair),
            EInt::Add(a, b) | EInt::Sub(a, b) => {
                self.int(a);
                self.int(b);
            }
        }
    }
}

impl Collector {
    fn mem(&mut self, mem: TokenOpaque<Mem>, eg: &EGraph) {
        match self.index.value(mem) {
            EMem::Init() => {}
            EMem::Store(mem, addr, val) => {
                self.mem(mem, eg);
                let addr = self.int(addr, eg);
                let val = self.int(val, eg);
                self.ir.push(Stmt::ST(addr, val));
            }
        }
        for pair in self.followed_by.get(&mem).cloned().into_iter().flatten() {
            let EPair(_, addr) = self.index.value(pair);
            let addr = self.int(addr, eg);
            let id = self.ir.len();
            self.ir.push(Stmt::LD(addr));
            self.pair2id.insert(pair, id);
        }
    }
    fn int(&mut self, int: TokenOpaque<Int>, eg: &EGraph) -> Id {
        if let Some(id) = self.int2id.get(&int) {
            return *id;
        }
        match self.index.value(int) {
            EInt::Cst(val) => self.ir.push(Stmt::Const(val.get(eg))),
            EInt::Var(name) => self.ir.push(Stmt::Var(name.get(eg))),
            EInt::LoadVal(pair) => return self.pair2id[&pair],
            EInt::Add(a, b) => {
                let a = self.int(a, eg);
                let b = self.int(b, eg);
                self.ir.push(Stmt::Add(a, b));
            }
            EInt::Sub(a, b) => {
                let a = self.int(a, eg);
                let b = self.int(b, eg);
                self.ir.push(Stmt::Sub(a, b));
            }
        }
        let id = self.ir.len() - 1;
        self.int2id.insert(int, id);
        id
    }
}

impl Memory {
    fn extract(&self, root: impl TokenOpaqueMarker) -> Index {
        Index::extract(
            &self,
            root,
            (self.init, self.store),
            self.load,
            (self.cst, self.var, self.load_val, self.add, self.sub),
        )
    }
}

impl IR {
    fn optimize(&self) -> Self {
        let mut m = Memory::default();
        let root = self.to_egraph(&mut m);

        while m.run_ruleset_active() {}
        let root = root.canon(&m);
        let index = m.extract(root);

        if true {
            let mut printer = IndexToString {
                m: &m,
                index: &index,
                out: Default::default(),
                v: Default::default(),
            };
            printer.mem(root);
            printer.eprint();
        }

        let r = Self::from_egraph(&m, index, root);

        r
    }
}

#[test]
fn no_stores() {
    let mut no_stores = IR::default();
    let addr1 = no_stores.var("addr1");
    let addr2 = no_stores.var("addr2");
    let c5 = no_stores.cst(5);
    let val1 = no_stores.ld(addr1);
    let val2 = no_stores.ld(addr2);
    let v1_v2 = no_stores.add(val1, val2);
    let v1_v2_c5 = no_stores.add(v1_v2, c5);
    let _r = no_stores.ld(v1_v2_c5);

    let opt = no_stores.optimize();
    assert!(opt.0.is_empty(), "{opt:?}");
}

#[test]
fn store_same_value() {
    let mut saw = IR::default();
    let base = saw.var("base");
    let addr1 = saw.ld(base);
    let addr2 = saw.ld(base);
    let val = saw.ld(addr1);
    saw.st(addr2, val);

    let opt = saw.optimize();
    assert!(opt.0.is_empty(), "{opt:?}");
}

#[test]
fn store_load_forwarding() {
    let ir = {
        let mut ir = IR::default();
        let c1 = ir.cst(1);
        let c10 = ir.cst(10);
        ir.st(c1, c10); // overwritten
        let x = ir.ld(c1); // x = 10
        let c2 = ir.cst(2);
        let y = ir.ld(c2);
        ir.st(c2, x);
        ir.st(c1, y); // overwrites
        ir
    };

    let expected1 = {
        let mut ir = IR::default();
        let c2 = ir.cst(2);
        let y = ir.ld(c2);
        let c10 = ir.cst(10);
        ir.st(c2, c10);
        let c1 = ir.cst(1);
        ir.st(c1, y);
        ir
    };
    let expected2 = {
        let mut ir = IR::default();
        let c2 = ir.cst(2);
        let y = ir.ld(c2);
        let c1 = ir.cst(1);
        ir.st(c1, y);
        let c10 = ir.cst(10);
        ir.st(c2, c10);
        ir
    };

    let opt = ir.optimize();
    assert!([expected1, expected2].contains(&opt), "{opt:?}");
}

#[test]
fn st_ld_forward_2() {
    let ir = {
        let mut ir = IR::default();
        let p = ir.var("p");
        let c1 = ir.cst(1);
        let p_a1 = ir.add(p, c1);
        let c5 = ir.cst(5);
        let c6 = ir.cst(6);
        ir.st(p_a1, c1); // overwritten
        let p_s1 = ir.sub(p, c1);
        ir.st(p_s1, c5);
        let x = ir.ld(p_a1); // c1
        let p_ax = ir.add(p, x); // p_a1
        ir.st(p_ax, c6); // overwrites
        ir
    };
    let opt = ir.optimize();

    eprintln!("opt: {opt:?}");
    assert_eq!(opt.0.len(), 8);
    let mut st = vec![];
    for stmt in opt.0.iter() {
        let Stmt::ST(addr, val) = stmt else {
            continue;
        };
        let val = match opt.0[*val] {
            Stmt::Const(v) => v,
            _ => panic!(),
        };
        let (is_add, a, b) = match opt.0[*addr] {
            Stmt::Add(a, b) => {
                if matches!(opt.0[a], Stmt::Var(_)) {
                    (true, a, b)
                } else {
                    (true, b, a)
                }
            }
            Stmt::Sub(a, b) => (false, a, b),
            _ => panic!(),
        };
        assert_eq!(opt.0[a], Stmt::Var("p".to_string()));
        assert_eq!(opt.0[b], Stmt::Const(1));
        st.push((val, is_add));
    }
    st.sort();
    assert_eq!(st, vec![(5, false), (6, true)]);
}
