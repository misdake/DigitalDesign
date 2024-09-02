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
impl<T: Oprand> CondOp<T> {
    pub fn invert(self) -> Self {
        match self {
            CondOp::Cmp(a, b, cond) => CondOp::Cmp(a, b, cond.invert()),
            CondOp::CmpI(a, i, cond) => CondOp::CmpI(a, i, cond.invert()),
        }
    }
    // pub(crate) fn convert<R: Oprand>(self, mut f: impl FnMut(T) -> R) -> CondOp<R> {
    //     match self {
    //         CondOp::Cmp(a, b, cond) => CondOp::Cmp(f(a), f(b), cond),
    //         CondOp::CmpI(a, i, cond) => CondOp::CmpI(f(a), i, cond),
    //     }
    // }
}
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
    /// pc + u4
    GetPc(u8),

    Mov(T),
    And(T, T),
    Or(T, T),
    Xor(T, T),
    Add(T, T),
    Addi(T, u8),
    Sub(T, T),
    Subi(T, u8),

    LoadMem(T, u8), // base, offset

    /// device, channel
    DeviceReceive(u8, u8),
}
impl<T: Oprand> ResultOp<T> {
    pub fn touch(&self, mut f: impl FnMut(&T, TouchType)) {
        self.convert(|v| f(&v, TouchType::Input));
    }
}
impl<T: Oprand> ResultOp<T> {
    pub fn convert<R: Oprand>(self, mut f: impl FnMut(T) -> R) -> ResultOp<R> {
        use ResultOp::*;
        match self {
            Inv(v) => Inv(f(v)),
            Neg(v) => Neg(f(v)),
            Not0(v) => Not0(f(v)),
            Cnt1(v) => Cnt1(f(v)),
            Log2(v) => Log2(f(v)),
            Mov(v) => Mov(f(v)),
            And(v1, v2) => And(f(v1), f(v2)),
            Or(v1, v2) => Or(f(v1), f(v2)),
            Xor(v1, v2) => Xor(f(v1), f(v2)),
            Add(v1, v2) => Add(f(v1), f(v2)),
            Addi(v, i) => Addi(f(v), i),
            Sub(v1, v2) => Sub(f(v1), f(v2)),
            Subi(v, i) => Subi(f(v), i),
            LoadMem(v, i) => LoadMem(f(v), i),
            GetPc(offset) => GetPc(offset),
            DeviceReceive(device, channel) => DeviceReceive(device, channel),
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub enum UpdateOp<T: Oprand> {
    /// halt with signal
    Halt(T),
    // unary
    Inv(T),
    Neg(T),
    Not0(T),
    Cnt1(T),
    Log2(T),
    // binary
    /// dst = dst << u4
    Lsl(T, u8),
    /// dst = dst >> u4
    Lsr(T, u8),
    /// dst = dst >>> u4
    Asr(T, u8),
    /// flags = compare(v1, v2)
    CmpReg(T, T),
    /// flags = compare(v, u4)
    CmpImm(T, u8),
    /// dst = pc + u4
    GetPc(T, u8),

    /// dst, src
    Mov(T, T),
    /// dst, src
    AndAssign(T, T),
    /// dst, src
    OrAssign(T, T),
    /// dst, src
    XorAssign(T, T),
    /// dst, src
    AddAssign(T, T),
    /// dst, value
    AddiAssign(T, u8),
    /// dst, src
    SubAssign(T, T),
    /// dst, value
    SubiAssign(T, u8),

    /// dst, value
    LoadImmLo(T, u8),
    /// dst, value
    LoadImmHi(T, u8),
    /// base, offset, value
    StoreMem(T, u8, T),
    /// base, offset, dst
    LoadMem(T, u8, T),

    /// device, channel, dst
    DeviceReceive(u8, u8, T),
    /// device, channel, src
    DeviceSend(u8, u8, T),
}
impl<T: Oprand> UpdateOp<T> {
    pub fn touch(&self, mut f: impl FnMut(&T, TouchType)) {
        self.convert(|v, _b| f(&v, TouchType::Input));
    }
}
impl<T: Oprand> UpdateOp<T> {
    /// f: FnMut(v, load_value)
    pub(crate) fn convert<R: Oprand>(self, mut f: impl FnMut(T, bool) -> R) -> UpdateOp<R> {
        use UpdateOp::*;
        match self {
            Halt(v) => Halt(f(v, true)),
            Inv(v) => Inv(f(v, true)),
            Neg(v) => Neg(f(v, true)),
            Not0(v) => Not0(f(v, true)),
            Cnt1(v) => Cnt1(f(v, true)),
            Log2(v) => Log2(f(v, true)),
            Lsl(dst, u4) => Lsl(f(dst, true), u4),
            Lsr(dst, u4) => Lsr(f(dst, true), u4),
            Asr(dst, u4) => Asr(f(dst, true), u4),
            CmpReg(v1, v2) => CmpReg(f(v1, true), f(v2, true)),
            CmpImm(v, u4) => CmpImm(f(v, true), u4),
            GetPc(dst, u4) => GetPc(f(dst, true), u4),
            LoadImmLo(v, i) => LoadImmLo(f(v, false), i),
            LoadImmHi(v, i) => LoadImmHi(f(v, true), i),
            Mov(dst, src) => Mov(f(dst, false), f(src, true)),
            AndAssign(dst, src) => AndAssign(f(dst, true), f(src, true)),
            OrAssign(dst, src) => OrAssign(f(dst, true), f(src, true)),
            XorAssign(dst, src) => XorAssign(f(dst, true), f(src, true)),
            AddAssign(dst, src) => AddAssign(f(dst, true), f(src, true)),
            AddiAssign(v, i) => AddiAssign(f(v, true), i),
            SubAssign(dst, src) => SubAssign(f(dst, true), f(src, true)),
            SubiAssign(v, i) => SubiAssign(f(v, true), i),
            StoreMem(base, i, value) => StoreMem(f(base, true), i, f(value, true)),
            LoadMem(base, i, dst) => LoadMem(f(base, true), i, f(dst, false)),

            DeviceReceive(device, channel, dst) => DeviceReceive(device, channel, f(dst, false)),
            DeviceSend(device, channel, src) => DeviceSend(device, channel, f(src, true)),
        }
    }
}
