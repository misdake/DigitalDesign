mod register_allocator1;
mod register_allocator2;
mod state_sync;

pub use register_allocator1::*;
pub use register_allocator2::*;
pub(crate) use state_sync::*;

use arrayvec::ArrayVec;
use std::collections::HashSet;
use std::fmt::{Debug, Formatter};

use crate::programmer::*;

#[derive(Copy, Clone, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub struct Reg(pub u8);

impl Debug for Reg {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("r{}", self.0))
    }
}

/// basic register operations with unlimited registers
/// calling convention is not considered (handled in later passes)
#[derive(Clone)]
pub enum RegisterOperation {
    /// basic ops with output
    Result(ResultOp<Reg>, Reg),
    Update(UpdateOp<Reg>),

    // recursive structures
    /// list of operations
    List(Vec<RegisterOperation>),
    /// condition, then, else
    If(
        CondOp<Reg>,
        Box<RegisterOperation>,
        Option<Box<RegisterOperation>>,
    ),
    /// condition, loop body
    Loop(CondOp<Reg>, Box<RegisterOperation>),

    // external flow control
    /// function name, return addr(output), params(output)
    Func(FuncName, Reg, ArrayVec<Reg, MAX_PARAM>),
    /// function name, params, living registers, return values(output)
    Call(
        FuncName,
        ArrayVec<Reg, MAX_PARAM>,
        HashSet<Reg>,
        ArrayVec<Reg, MAX_RETURN>,
    ),
    /// return addr, return values, ever allocated registers
    Return(Reg, ArrayVec<Reg, MAX_RETURN>, HashSet<Reg>),
}
impl RegisterOperation {
    pub fn vec_to_box_ra(mut list: Vec<RegisterOperation>) -> Option<Box<RegisterOperation>> {
        match list.len() {
            0 => None,
            1 => Some(Box::new(list.remove(0))),
            _ => Some(Box::new(RegisterOperation::List(list))),
        }
    }
}

impl Debug for RegisterOperation {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            RegisterOperation::Result(op, r0) => {
                f.write_fmt(format_args!("Result({op:?}, {r0:?})"))
            }
            RegisterOperation::Update(op) => f.write_fmt(format_args!("Update({op:?})")),
            RegisterOperation::List(list) => f.debug_list().entries(list).finish(),
            RegisterOperation::If(cond, t, e) => {
                let mut f = f.debug_struct("If");
                f.field("cond", cond).field("then", t);
                if let Some(e) = e {
                    f.field("else", e);
                }
                f.finish()
            }
            RegisterOperation::Loop(cond, l) => f
                .debug_struct("Loop")
                .field("cond", cond)
                .field("body", l)
                .finish(),
            RegisterOperation::Func(name, ra, params) => {
                f.write_fmt(format_args!("Func({name}, {ra:?}, {params:?})"))
            }
            RegisterOperation::Call(name, params, living_regs, return_values) => f.write_fmt(
                format_args!("Call({name}, {params:?}, {return_values:?}) living: {living_regs:?}"),
            ),
            RegisterOperation::Return(ra, return_values, ever_allocated_regs) => {
                f.write_fmt(format_args!(
                    "Return({ra:?}, {return_values:?}) ever_allocated: {ever_allocated_regs:?}"
                ))
            }
        }
    }
}

impl RegisterOperation {
    // //TODO use assembler
    // pub fn into_inst(self) -> Vec<Instruction> {
    //     fn i4_to_u4(i4: i8) -> u8 {
    //         assert!(i4 >= -8);
    //         assert!(i4 <= 7);
    //         (i4 as u8) & 0b1111
    //     }
    //
    //     let mut r = vec![];
    //     match self {
    //         RegisterOperation1::Result(op, r0) => match op {
    //             ResultOp::Add(r1, r2) => r.push(Instruction::add(r2.0, r1.0, r0.0)),
    //             ResultOp::Addi(r1, i4) => r.push(Instruction::addi(r1.0, i4_to_u4(i4), r0.0)),
    //         },
    //         RegisterOperation1::Update(op) => match op {
    //             UpdateOp::Mov(r0, r1) => {
    //                 if r0 != r1 {
    //                     r.push(Instruction::mov(r1.0, r0.0))
    //                 }
    //             }
    //             UpdateOp::LoadImmLo(r0, u8) => {
    //                 let hi = u8 >> 4;
    //                 let lo = u8 & 0b1111;
    //                 r.push(Instruction::load_lo(hi, lo, r0.0))
    //             }
    //             UpdateOp::LoadImmHi(r0, u8) => {
    //                 let hi = u8 >> 4;
    //                 let lo = u8 & 0b1111;
    //                 r.push(Instruction::load_hi(hi, lo, r0.0))
    //             }
    //             UpdateOp::AddAssign(r0, r1) => r.push(Instruction::add(r0.0, r1.0, r0.0)),
    //             UpdateOp::AddiAssign(r0, i4) => r.push(Instruction::addi(r0.0, i4_to_u4(i4), r0.0)),
    //         },
    //
    //         RegisterOperation1::List(list) => {
    //             for op in list {
    //                 r.extend(op.into_inst())
    //             }
    //         }
    //         RegisterOperation1::If(_, _, _) => todo!(),
    //         RegisterOperation1::Loop(_, _) => todo!(),
    //
    //         RegisterOperation1::Func(_, _, _) => {
    //             // todo!()
    //         }
    //         RegisterOperation1::Call(_, _, _) => {
    //             // todo!()
    //         }
    //         RegisterOperation1::Return(_, _) => {
    //             // todo!()
    //         }
    //     }
    //     r
    // }
}
