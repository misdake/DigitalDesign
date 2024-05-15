use crate::programmer::*;
use arrayvec::ArrayVec;

#[derive(Clone, Debug)]
pub(crate) enum VariableOperation3 {
    /// alloc new variable
    Alloc(Variable),
    /// create new variable with operation inputs
    Result(ResultOp<Variable>),
    /// update value of variable
    Update(UpdateOp<Variable>),
    /// write last alloc op result to this variable
    Write(Variable),
    /// after last result/update usage
    Free(Variable),

    // recursive structures
    /// list of operations
    List(Vec<VariableOperation2Scope>),
    /// condition, then, else
    If(
        Box<VariableOperation2Scope<CondOp<Variable>>>,
        Box<VariableOperation2Scope>,
        Option<Box<VariableOperation2Scope>>,
    ),
    /// condition, loop body
    Loop(
        Box<VariableOperation2Scope<CondOp<Variable>>>,
        Box<VariableOperation2Scope>,
    ),

    // external flow control
    /// function name, params(output)
    Func(FuncName, ArrayVec<Variable, MAX_PARAM>),
    /// function name, params, return addr(output), return values(output)
    Call(
        FuncName,
        ArrayVec<Variable, MAX_PARAM>,
        Variable,
        ArrayVec<Variable, MAX_RETURN>,
    ),
    /// return addr, return values
    Return(Variable, ArrayVec<Variable, MAX_RETURN>),
}

//TODO generate from VariableOperation2Scope
