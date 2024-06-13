use crate::isa::Cond;
use std::hash::Hash;

#[derive(Copy, Clone, Eq, PartialEq)]
pub enum TouchType {
    Input,
    Output,
    UserAlloc,
}

pub trait Oprand: Copy + Clone + Hash + Eq + PartialEq {}
impl<T> Oprand for T where T: Copy + Clone + Hash + Eq + PartialEq {}

/// compare .0 with .1
///   for example a > 2 becomes Cmp(a, 2, Cond::Greater)
#[derive(Copy, Clone, Debug)]
pub enum CondOp<T: Oprand> {
    Cmp(T, T, Cond),
    CmpI(T, u8, Cond),
}
// impl<T: Oprand> CondOp<T> {
//     pub(crate) fn convert<R: Oprand>(self, mut f: impl FnMut(T) -> R) -> CondOp<R> {
//         match self {
//             CondOp::Cmp(a, b, cond) => CondOp::Cmp(f(a), f(b), cond),
//             CondOp::CmpI(a, i, cond) => CondOp::CmpI(f(a), i, cond),
//         }
//     }
// }
impl<T: Oprand> CondOp<T> {
    pub fn touch(&self, mut f: impl FnMut(&T, TouchType)) {
        match self {
            CondOp::Cmp(a, b, _) => {
                f(a, TouchType::Input);
                f(b, TouchType::Input);
            }
            CondOp::CmpI(a, _, _) => f(a, TouchType::Input),
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub enum ResultOp<T: Oprand> {
    // unary
    Inv(T),
    Neg(T),
    Not0(T),
    Cnt1(T),
    Log2(T),

    Add(T, T),
    Addi(T, u8),
    LoadMem(T, u8), // base, offset
}
impl<T: Oprand> ResultOp<T> {
    pub fn touch(&self, mut f: impl FnMut(&T, TouchType)) {
        self.convert(|v| f(&v, TouchType::Input));
    }
}
impl<T: Oprand> ResultOp<T> {
    pub fn convert<R: Oprand>(self, mut f: impl FnMut(T) -> R) -> ResultOp<R> {
        match self {
            ResultOp::Inv(v) => ResultOp::Inv(f(v)),
            ResultOp::Neg(v) => ResultOp::Neg(f(v)),
            ResultOp::Not0(v) => ResultOp::Not0(f(v)),
            ResultOp::Cnt1(v) => ResultOp::Cnt1(f(v)),
            ResultOp::Log2(v) => ResultOp::Log2(f(v)),
            ResultOp::Add(v1, v2) => ResultOp::Add(f(v1), f(v2)),
            ResultOp::Addi(v, i) => ResultOp::Addi(f(v), i),
            ResultOp::LoadMem(v, i) => ResultOp::LoadMem(f(v), i),
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub enum UpdateOp<T: Oprand> {
    // unary
    Inv(T),
    Neg(T),
    Not0(T),
    Cnt1(T),
    Log2(T),

    /// dst, value
    LoadImmLo(T, u8),
    /// dst, value
    LoadImmHi(T, u8),
    /// dst, src
    Mov(T, T),
    /// dst, src
    AddAssign(T, T),
    /// dst, value
    AddiAssign(T, u8),
    /// dst, value
    SubiAssign(T, u8),
    /// base, offset, value
    StoreMem(T, u8, T),
    /// just halt
    Halt(),
}
impl<T: Oprand> UpdateOp<T> {
    pub fn touch(&self, mut f: impl FnMut(&T, TouchType)) {
        self.convert(|v, _b| f(&v, TouchType::Input));
    }
}
impl<T: Oprand> UpdateOp<T> {
    /// f: FnMut(v, load_value)
    pub(crate) fn convert<R: Oprand>(self, mut f: impl FnMut(T, bool) -> R) -> UpdateOp<R> {
        match self {
            UpdateOp::Inv(v) => UpdateOp::Inv(f(v, true)),
            UpdateOp::Neg(v) => UpdateOp::Neg(f(v, true)),
            UpdateOp::Not0(v) => UpdateOp::Not0(f(v, true)),
            UpdateOp::Cnt1(v) => UpdateOp::Cnt1(f(v, true)),
            UpdateOp::Log2(v) => UpdateOp::Log2(f(v, true)),
            UpdateOp::LoadImmLo(v, i) => UpdateOp::LoadImmLo(f(v, false), i),
            UpdateOp::LoadImmHi(v, i) => UpdateOp::LoadImmHi(f(v, true), i),
            UpdateOp::Mov(dst, src) => UpdateOp::Mov(f(dst, false), f(src, true)),
            UpdateOp::AddAssign(dst, src) => UpdateOp::AddAssign(f(dst, true), f(src, true)),
            UpdateOp::AddiAssign(v, i) => UpdateOp::AddiAssign(f(v, true), i),
            UpdateOp::SubiAssign(v, i) => UpdateOp::SubiAssign(f(v, true), i),
            UpdateOp::StoreMem(base, i, value) => {
                UpdateOp::StoreMem(f(base, true), i, f(value, true))
            }
            UpdateOp::Halt() => UpdateOp::Halt(),
        }
    }
}
