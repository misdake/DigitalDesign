use std::fmt::{Debug, Formatter, Write};
use std::hash::Hash;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering::Relaxed;

static NEXT_VARIABLE: AtomicUsize = AtomicUsize::new(0);

/// basic element of DSL
#[derive(Copy, Clone, Hash, Eq, PartialEq)]
pub struct Variable(usize);

impl Default for Variable {
    fn default() -> Self {
        Self::new()
    }
}
impl Variable {
    pub fn new() -> Self {
        Self(NEXT_VARIABLE.fetch_add(1, Relaxed))
    }
    pub fn reset() {
        NEXT_VARIABLE.store(1, Relaxed)
    }
}

impl Debug for Variable {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        const A: u8 = b'a';
        let mut curr = self.0;
        let mut s = String::new();
        if curr == 0 {
            return f.write_char('a');
        }
        while curr > 0 {
            let c = (curr % 26) as u8;
            curr /= 26;
            s.push((A + c) as char);
        }
        f.write_str(&s)
    }
}

#[rustfmt::skip]
#[macro_export]
macro_rules! cmp {
    ($v: ident > $i: literal) => { CondOp::CmpI($v, $i, Cond::Greater) };
    ($v: ident >= $i: literal) => { CondOp::CmpI($v, $i, Cond::GreaterEqual) };
    ($v: ident < $i: literal) => { CondOp::CmpI($v, $i, Cond::Less) };
    ($v: ident <= $i: literal) => { CondOp::CmpI($v, $i, Cond::LessEqual) };
    ($v: ident == $i: literal) => { CondOp::CmpI($v, $i, Cond::Equal) };
    ($v: ident != $i: literal) => { CondOp::CmpI($v, $i, Cond::NotEqual) };

    ($v: ident > $i: ident) => { CondOp::Cmp($v, $i, Cond::Greater) };
    ($v: ident >= $i: ident) => { CondOp::Cmp($v, $i, Cond::GreaterEqual) };
    ($v: ident < $i: ident) => { CondOp::Cmp($v, $i, Cond::Less) };
    ($v: ident <= $i: ident) => { CondOp::Cmp($v, $i, Cond::LessEqual) };
    ($v: ident == $i: ident) => { CondOp::Cmp($v, $i, Cond::Equal) };
    ($v: ident != $i: ident) => { CondOp::Cmp($v, $i, Cond::NotEqual) };
}
