mod register_allocator1;
mod register_allocator2;
mod register_usages;
mod state_sync;

pub use register_allocator1::*;
pub use register_allocator2::*;
pub use register_usages::*;
pub(crate) use state_sync::*;

use arrayvec::ArrayVec;
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
    /// function name, params, return values(output)
    Call(
        FuncName,
        ArrayVec<Reg, MAX_PARAM>, // TODO variables that are freed at call (before return)?
        ArrayVec<Reg, MAX_RETURN>,
    ),
    /// return addr, return values
    Return(Reg, ArrayVec<Reg, MAX_RETURN>),
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
            RegisterOperation::Call(name, params, return_values) => {
                f.write_fmt(format_args!("Call({name}, {params:?}, {return_values:?})"))
            }
            RegisterOperation::Return(ra, return_values) => {
                f.write_fmt(format_args!("Return({ra:?}, {return_values:?})"))
            }
        }
    }
}
