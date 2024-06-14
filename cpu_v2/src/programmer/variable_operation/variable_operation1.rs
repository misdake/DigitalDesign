use crate::programmer::*;
use arrayvec::ArrayVec;

pub const MAX_RETURN: usize = 2;
pub const MAX_PARAM: usize = 4;
pub type FuncName = &'static str;

pub type FuncParams = ArrayVec<Variable, MAX_PARAM>;
pub type ReturnValues = ArrayVec<Variable, MAX_RETURN>;
pub fn func_params<const C: usize>(params: [Variable; C]) -> FuncParams {
    let mut r = ArrayVec::new();
    r.extend(params.into_iter());
    r
}
pub fn return_values<const C: usize>(params: [Variable; C]) -> ReturnValues {
    let mut r = ArrayVec::new();
    r.extend(params.into_iter());
    r
}

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
    /// while condition, loop body
    Loop(CondOp<Variable>, Box<VariableOperation1>), //TODO support continue and break

    // external flow control
    /// function name, return addr(output), params(output)
    Func(FuncName, Variable, FuncParams),
    /// function name, params, return values(output)
    Call(FuncName, FuncParams, ReturnValues),
    /// return addr, return values
    Return(Variable, ReturnValues),
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

#[cfg(test)]
pub(crate) fn vo1_basic_program() -> (VariableOperation1, FuncDecl) {
    let a = Variable::new();
    let b = Variable::new();
    let c = Variable::new();
    let d = Variable::new();
    let e = Variable::new();

    let ra = Variable::new();

    let vo1 = VariableOperation1::List(vec![
        VariableOperation1::Func("add", ra, func_params([])),
        VariableOperation1::Update(UpdateOp::LoadImmLo(a, 1)),
        VariableOperation1::Update(UpdateOp::LoadImmLo(b, 1)),
        VariableOperation1::Call("print", func_params([a]), return_values([])),
        VariableOperation1::Result(ResultOp::Add(a, b), c),
        VariableOperation1::Result(ResultOp::Add(b, c), d),
        VariableOperation1::Result(ResultOp::Add(c, d), e),
        VariableOperation1::Call("print", func_params([e]), return_values([])),
        VariableOperation1::Return(ra, return_values([e])),
    ]);
    let decl = FuncDecl::new("try", &[], &["r"]);
    (vo1, decl)
}

#[cfg(test)]
pub(crate) fn vo1_func_program() -> (VariableOperation1, FuncDecl) {
    let a = Variable::new();
    let b = Variable::new();
    let c = Variable::new();

    let ra = Variable::new();

    let vo1 = VariableOperation1::List(vec![
        VariableOperation1::Func("add", ra, func_params([a, b])),
        VariableOperation1::Alloc(c),
        VariableOperation1::Result(ResultOp::Add(a, b), c),
        VariableOperation1::Return(ra, return_values([c])),
    ]);
    let decl = FuncDecl::new("add", &["a", "b"], &["c"]);
    (vo1, decl)
}

#[cfg(test)]
pub(crate) fn vo1_call_program(x: u8, y: u8) -> (VariableOperation1, FuncDecl) {
    let a = Variable::new();
    let b = Variable::new();
    let r = Variable::new();
    let ra = Variable::new();

    let vo1 = VariableOperation1::List(vec![
        VariableOperation1::Func("call", ra, func_params([])),
        VariableOperation1::Alloc(a),
        VariableOperation1::Alloc(b),
        VariableOperation1::Update(UpdateOp::LoadImmLo(a, x)),
        VariableOperation1::Update(UpdateOp::LoadImmLo(b, y)),
        VariableOperation1::Call("add", func_params([a, b]), return_values([r])),
        VariableOperation1::Update(UpdateOp::Halt()),
    ]);
    let decl = FuncDecl::new("call", &[], &["r"]);
    (vo1, decl)
}

#[cfg(test)]
pub(crate) fn vo1_if_program() -> (VariableOperation1, FuncDecl) {
    use crate::isa::Cond;

    let a = Variable::new();
    let b = Variable::new();
    let c = Variable::new();
    let d = Variable::new();
    let ra = Variable::new();

    let func = VariableOperation1::Func("if", ra, func_params([]));

    let init = VariableOperation1::List(vec![
        VariableOperation1::Alloc(a),
        VariableOperation1::Alloc(b),
        VariableOperation1::Alloc(c),
        VariableOperation1::Alloc(d),
        VariableOperation1::Update(UpdateOp::LoadImmLo(a, 10)),
        VariableOperation1::Update(UpdateOp::LoadImmLo(b, 20)),
        VariableOperation1::Update(UpdateOp::LoadImmLo(c, 2)),
        VariableOperation1::Update(UpdateOp::LoadImmLo(d, 30)),
    ]);
    let if_block = VariableOperation1::If(
        CondOp::CmpI(c, 1, Cond::Greater),
        Box::new(VariableOperation1::Update(UpdateOp::Mov(d, a))),
        Some(Box::new(VariableOperation1::Update(UpdateOp::Mov(d, b)))),
    );
    let result = VariableOperation1::Update(UpdateOp::LoadImmHi(d, 1));
    let ret = VariableOperation1::Return(ra, return_values([d]));
    let vo1 = VariableOperation1::List(vec![func, init, if_block, result, ret]);
    let decl = FuncDecl::new("if", &[], &["d"]);
    (vo1, decl)
}

#[cfg(test)]
pub(crate) fn vo1_loop_program() -> (VariableOperation1, FuncDecl) {
    use crate::isa::Cond;

    let s = Variable::new();
    let i = Variable::new();
    let ra = Variable::new();

    let func = VariableOperation1::Func("loop", ra, func_params([]));

    let init = VariableOperation1::List(vec![
        VariableOperation1::Alloc(s),
        VariableOperation1::Alloc(i),
        VariableOperation1::Update(UpdateOp::LoadImmLo(s, 0)),
        VariableOperation1::Update(UpdateOp::LoadImmLo(i, 1)),
    ]);
    let loop_block = VariableOperation1::Loop(
        CondOp::CmpI(i, 10, Cond::LessEqual),
        Box::new(VariableOperation1::List(vec![
            VariableOperation1::Update(UpdateOp::AddAssign(s, i)),
            VariableOperation1::Update(UpdateOp::AddiAssign(i, 1)),
        ])),
    );
    let ret = VariableOperation1::Return(ra, return_values([s]));

    let vo1 = VariableOperation1::List(vec![func, init, loop_block, ret]);
    let decl = FuncDecl::new("loop", &[], &["sum"]);
    (vo1, decl)
}

#[cfg(test)]
pub(crate) fn vo1_spill_program(n: usize, pass: usize) -> (VariableOperation1, FuncDecl) {
    let ra = Variable::new();
    let mut v = vec![];
    let mut list = vec![];
    list.push(VariableOperation1::Func("loop", ra, func_params([])));
    for i in 0..n {
        v.push(Variable::new());
        list.push(VariableOperation1::Alloc(v[i]));
        list.push(VariableOperation1::Update(UpdateOp::LoadImmLo(
            v[i], i as u8,
        )));
    }

    for _ in 0..pass {
        for i in 1..n {
            list.push(VariableOperation1::Update(UpdateOp::AddAssign(
                v[i],
                v[i - 1],
            )));
        }
    }

    list.push(VariableOperation1::Return(ra, return_values([v[n - 1]])));
    let vo1 = VariableOperation1::List(list);
    let decl = FuncDecl::new("spill", &[], &["sum"]);
    (vo1, decl)
}
