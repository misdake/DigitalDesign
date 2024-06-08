use std::fmt::{Debug, Formatter, Write};
use std::hash::Hash;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering::Relaxed;

static NEXT_VARIABLE: AtomicUsize = AtomicUsize::new(0);

/// basic element of DSL
#[derive(Copy, Clone, Hash, Eq, PartialEq)]
pub struct Variable(usize);
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
