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
    Add(T, T),
    Addi(T, u8),
    LoadMem(T, u8), // base, offset
}
impl<T: Oprand> ResultOp<T> {
    pub fn touch(&self, mut f: impl FnMut(&T, TouchType)) {
        match self {
            ResultOp::Add(v1, v2) => {
                f(v1, TouchType::Input);
                f(v2, TouchType::Input);
            }
            ResultOp::Addi(v, _) => {
                f(v, TouchType::Input);
            }
            ResultOp::LoadMem(base, _) => f(base, TouchType::Input),
        }
    }
}
// impl<T: Oprand> ResultOp<T> {
//     pub fn convert<R: Oprand>(self, mut f: impl FnMut(T) -> R) -> ResultOp<R> {
//         match self {
//             ResultOp::Add(v1, v2) => ResultOp::Add(f(v1), f(v2)),
//             ResultOp::Addi(v, i) => ResultOp::Addi(f(v), i),
//             ResultOp::LoadMem(v, i) => ResultOp::LoadMem(f(v), i),
//         }
//     }
// }

#[derive(Copy, Clone, Debug)]
pub enum UpdateOp<T: Oprand> {
    /// dst, value
    LoadImmLo(T, u8), //TODO actually this is a resultop
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
}
impl<T: Oprand> UpdateOp<T> {
    pub fn touch(&self, mut f: impl FnMut(&T, TouchType)) {
        match self {
            UpdateOp::LoadImmLo(v, _) => f(v, TouchType::Input),
            UpdateOp::LoadImmHi(v, _) => f(v, TouchType::Input),
            UpdateOp::Mov(dst, src) => {
                f(dst, TouchType::Input);
                f(src, TouchType::Input);
            }
            UpdateOp::AddAssign(dst, src) => {
                f(dst, TouchType::Input);
                f(src, TouchType::Input);
            }
            UpdateOp::AddiAssign(v, _) => f(v, TouchType::Input),
            UpdateOp::SubiAssign(v, _) => f(v, TouchType::Input),
            UpdateOp::StoreMem(base, _, value) => {
                f(base, TouchType::Input);
                f(value, TouchType::Input);
            }
        }
    }
}
// impl<T: Oprand> UpdateOp<T> {
//     pub(crate) fn convert<R: Oprand>(self, mut f: impl FnMut(T) -> R) -> UpdateOp<R> {
//         match self {
//             UpdateOp::LoadImmLo(v, i) => UpdateOp::LoadImmLo(f(v), i),
//             UpdateOp::LoadImmHi(v, i) => UpdateOp::LoadImmHi(f(v), i),
//             UpdateOp::Mov(dst, src) => UpdateOp::Mov(f(dst), f(src)),
//             UpdateOp::AddAssign(dst, src) => UpdateOp::AddAssign(f(dst), f(src)),
//             UpdateOp::AddiAssign(v, i) => UpdateOp::AddiAssign(f(v), i),
//             UpdateOp::SubiAssign(v, i) => UpdateOp::SubiAssign(f(v), i),
//             UpdateOp::StoreMem(base, i, value) => UpdateOp::StoreMem(f(base), i, f(value)),
//         }
//     }
// }
