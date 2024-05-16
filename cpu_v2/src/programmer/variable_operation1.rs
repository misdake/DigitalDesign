use crate::programmer::*;
use arrayvec::ArrayVec;

pub const MAX_RETURN: usize = 4;
pub const MAX_PARAM: usize = 4;
pub type FuncName = &'static str;

/// basic operations generated directly from DSL
#[derive(Clone, Debug)]
pub enum VariableOperation1 {
    // basic linear operations
    /// result(output)
    Alloc(Variable),
    /// op, result(output)
    Result(ResultOp<Variable>, Variable),
    /// overwrite value
    Update(UpdateOp<Variable>),

    // recursive structures
    /// list of operations
    List(Vec<VariableOperation1>),
    /// condition, then, else
    If(
        CondOp<Variable>,
        Box<VariableOperation1>,
        Option<Box<VariableOperation1>>,
    ),
    /// condition, loop body
    Loop(CondOp<Variable>, Box<VariableOperation1>), //TODO support continue and break

    // external flow control
    /// function name, return addr(output), params(output)
    Func(FuncName, Variable, ArrayVec<Variable, MAX_PARAM>),
    /// function name, params, return values(output)
    Call(
        FuncName,
        ArrayVec<Variable, MAX_PARAM>,
        ArrayVec<Variable, MAX_RETURN>,
    ),
    /// return addr, return values
    Return(Variable, ArrayVec<Variable, MAX_RETURN>),
}

impl VariableOperation1 {
    pub fn touch_primitive(&self, mut f: impl FnMut(&Variable, TouchType)) {
        match self {
            VariableOperation1::Alloc(v) => f(v, TouchType::UserAlloc),
            VariableOperation1::Result(op, v) => {
                op.touch(&mut f);
                f(v, TouchType::Output);
            }
            VariableOperation1::Update(op) => op.touch(&mut f),
            _ => {}
        }
    }
}
