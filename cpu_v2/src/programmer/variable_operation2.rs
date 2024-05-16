use crate::programmer::*;
use arrayvec::ArrayVec;
use std::collections::HashSet;
use std::ops::Sub;

#[derive(Clone, Debug)]
pub struct VariableOperation2Scope<Op = VariableOperation2> {
    op: Op,
    info: ScopeInfo,
}

/// VariableOperation1 with scope input/output info
#[derive(Clone, Debug)]
pub enum VariableOperation2 {
    // basic linear operations
    /// result(output)
    Alloc(Variable),
    /// op, result(output)
    Result(ResultOp<Variable>, Variable),
    /// overwrite value
    Update(UpdateOp<Variable>),

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

#[derive(Default, Clone, Debug)]
pub struct ScopeInfo {
    // first pass:
    /// all used inputs
    inputs: HashSet<Variable>,
    /// all allocated variables that can be exported (not necessarily used)
    possible_outputs: HashSet<Variable>,

    // second pass:
    /// inputs not in living after, filled in second_pass
    inputs_drop_after: HashSet<Variable>,
    /// outputs that are used later, subset of all_outputs, filled in second_pass
    real_outputs: HashSet<Variable>,
}

impl VariableOperation2Scope<VariableOperation2> {
    pub fn from(op: VariableOperation1) -> Self {
        let mut this = Self::from_raw(op);
        this.second_pass(&mut HashSet::new());
        this
    }
    fn from_raw(op: VariableOperation1) -> Self {
        match op {
            op @ VariableOperation1::Alloc(_)
            | op @ VariableOperation1::Result(_, _)
            | op @ VariableOperation1::Update(_) => Self::basic(op),
            VariableOperation1::List(list) => {
                let list = list
                    .into_iter()
                    .map(|item| Self::from_raw(item))
                    .collect::<Vec<_>>();
                Self::list(list)
            }
            VariableOperation1::If(cond, if_block, else_block) => {
                let if_block = Self::from_raw(*if_block);
                let else_block = else_block.map(|op| Self::from_raw(*op));
                Self::if_block(cond, if_block, else_block)
            }
            VariableOperation1::Loop(cond, loop_block) => {
                let loop_block = Self::from_raw(*loop_block);
                Self::loop_block(cond, loop_block)
            }
            VariableOperation1::Func(name, ra, param) => Self::func(name, ra, param),
            VariableOperation1::Call(name, param, rv) => Self::call(name, param, rv),
            VariableOperation1::Return(ra, rv) => Self::ret(ra, rv),
        }
    }

    fn basic(op: VariableOperation1) -> Self {
        let mut inputs = HashSet::new();
        let mut possible_outputs = HashSet::new();

        // touch top level
        op.touch_primitive(|v, ty| match ty {
            TouchType::Input => {
                inputs.insert(*v);
            }
            TouchType::Output => {
                possible_outputs.insert(*v);
            }
            TouchType::UserAlloc => {
                possible_outputs.insert(*v);
            }
        });

        let op = match op {
            VariableOperation1::Alloc(r) => VariableOperation2::Alloc(r),
            VariableOperation1::Result(op, r) => VariableOperation2::Result(op, r),
            VariableOperation1::Update(op) => VariableOperation2::Update(op),
            _ => {
                unreachable!()
            }
        };

        Self {
            op,
            info: ScopeInfo {
                inputs,
                possible_outputs,
                ..Default::default()
            },
        }
    }
    fn list(list: Vec<VariableOperation2Scope>) -> Self {
        let mut inputs = HashSet::new();
        let mut possible_outputs = HashSet::new();

        for op in &list {
            for output in &op.info.possible_outputs {
                possible_outputs.insert(*output);
            }
            for v in &op.info.inputs {
                if !possible_outputs.contains(v) {
                    inputs.insert(*v); // only outer inputs
                }
            }
        }

        Self {
            op: VariableOperation2::List(list),
            info: ScopeInfo {
                inputs,
                possible_outputs,
                ..Default::default()
            },
        }
    }
    fn if_block(
        cond: CondOp<Variable>,
        then_block: VariableOperation2Scope,
        else_block: Option<VariableOperation2Scope>,
    ) -> Self {
        // inputs from cond and two blocks
        let mut cond_inputs = HashSet::new();
        cond.touch(|v, ty| match ty {
            TouchType::Input => {
                cond_inputs.insert(*v);
            }
            _ => {
                unreachable!("no alloc or output allowed in CondOp")
            }
        });
        let mut if_inputs = cond_inputs.clone();
        if_inputs.extend(then_block.info.inputs.iter());
        if let Some(else_block) = &else_block {
            if_inputs.extend(else_block.info.inputs.iter());
        }
        // no output

        let cond: VariableOperation2Scope<CondOp<Variable>> = VariableOperation2Scope {
            op: cond,
            info: ScopeInfo {
                inputs: cond_inputs,
                ..Default::default()
            },
        };

        Self {
            op: VariableOperation2::If(
                Box::new(cond),
                Box::new(then_block),
                else_block.map(Box::new),
            ),
            info: ScopeInfo {
                inputs: if_inputs,
                ..Default::default()
            },
        }
    }
    fn loop_block(cond: CondOp<Variable>, loop_block: VariableOperation2Scope) -> Self {
        // inputs from cond and loop block
        let mut cond_inputs = HashSet::new();
        cond.touch(|v, ty| match ty {
            TouchType::Input => {
                cond_inputs.insert(*v);
            }
            _ => {
                unreachable!("no alloc or output allowed in CondOp")
            }
        });
        let mut loop_inputs = cond_inputs.clone();
        loop_inputs.extend(loop_block.info.inputs.iter());
        // no output

        let cond: VariableOperation2Scope<CondOp<Variable>> = VariableOperation2Scope {
            op: cond,
            info: ScopeInfo {
                inputs: cond_inputs,
                ..Default::default()
            },
        };

        Self {
            op: VariableOperation2::Loop(Box::new(cond), Box::new(loop_block)),
            info: ScopeInfo {
                inputs: loop_inputs,
                ..Default::default()
            },
        }
    }
    fn func(
        name: &'static str,
        return_addr: Variable,
        params: ArrayVec<Variable, MAX_PARAM>,
    ) -> Self {
        let mut possible_outputs = HashSet::new();
        possible_outputs.insert(return_addr);
        for param in &params {
            possible_outputs.insert(*param);
        }
        // no output

        Self {
            op: VariableOperation2::Func(name, return_addr, params),
            info: ScopeInfo {
                possible_outputs,
                ..Default::default()
            },
        }
    }
    fn call(
        name: &'static str,
        params: ArrayVec<Variable, MAX_PARAM>,
        rv: ArrayVec<Variable, MAX_RETURN>,
    ) -> Self {
        let mut inputs = HashSet::new();
        let mut possible_outputs = HashSet::new();

        for param in &params {
            inputs.insert(*param);
        }
        for rv in &rv {
            possible_outputs.insert(*rv);
        }

        Self {
            op: VariableOperation2::Call(name, params, rv),
            info: ScopeInfo {
                inputs,
                possible_outputs,
                ..Default::default()
            },
        }
    }
    fn ret(ra: Variable, rv: ArrayVec<Variable, MAX_RETURN>) -> Self {
        let mut inputs = HashSet::new();
        for param in &rv {
            inputs.insert(*param);
        }
        inputs.insert(ra);

        Self {
            op: VariableOperation2::Return(ra, rv),
            info: ScopeInfo {
                inputs,
                ..Default::default()
            },
        }
    }
}

impl ScopeInfo {
    fn second_pass1(&mut self, later_inputs: &mut HashSet<Variable>) {
        self.inputs_drop_after = self.inputs.sub(later_inputs);

        self.real_outputs = self
            .possible_outputs
            .drain()
            .filter(|v| later_inputs.contains(v))
            .collect();
    }
    fn second_pass2(&mut self, later_inputs: &mut HashSet<Variable>) {
        later_inputs.extend(self.inputs.iter().cloned());
        for output in &self.possible_outputs {
            later_inputs.remove(output);
        }
    }
}
impl VariableOperation2Scope {
    fn second_pass(&mut self, later_inputs: &mut HashSet<Variable>) {
        self.info.second_pass1(later_inputs);

        #[allow(clippy::single_match)]
        match &mut self.op {
            VariableOperation2::List(list) => {
                // bottom to top, collect inputs
                for scope in list.iter_mut().rev() {
                    scope.second_pass(later_inputs);
                }
            }
            VariableOperation2::If(cond, then_block, else_block) => {
                if let Some(else_block) = else_block {
                    else_block.second_pass(&mut later_inputs.clone());
                }
                then_block.second_pass(&mut later_inputs.clone());

                if let Some(else_block) = else_block {
                    later_inputs.extend(else_block.info.inputs.iter());
                }
                later_inputs.extend(then_block.info.inputs.iter());

                cond.info.second_pass1(later_inputs);
                cond.info.second_pass2(later_inputs);
            }
            VariableOperation2::Loop(cond, loop_block) => {
                // treat inputs as later_inputs
                later_inputs.extend(cond.info.inputs.iter());
                later_inputs.extend(loop_block.info.inputs.iter());

                loop_block.second_pass(later_inputs);
                loop_block.info.inputs_drop_after.clear();

                cond.info.second_pass1(later_inputs);
                cond.info.second_pass2(later_inputs);
                cond.info.inputs_drop_after.clear();
            }
            _ => {}
        }

        self.info.second_pass2(later_inputs);
    }
}

#[test]
fn test_vo2s() {
    use crate::isa::Cond;

    let a = Variable::new();
    let b = Variable::new();
    let c = Variable::new();
    let d = Variable::new();

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
    let mut r = ArrayVec::<Variable, MAX_RETURN>::new();
    r.push(d);
    let ret = VariableOperation1::Return(d, r);
    let all = VariableOperation1::List(vec![init, if_block, result, ret]);

    // println!("RawOperation: {:#?}", all);
    let scope = VariableOperation2Scope::from(all);

    println!("RawOperationScope: {:#?}", scope);
}
#[test]
fn test_vo2s_2() {
    use crate::isa::Cond;

    let s = Variable::new();
    let i = Variable::new();
    let ra = Variable::new();

    let func = VariableOperation1::Func("sum", ra, ArrayVec::new());

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
    let mut r = ArrayVec::<Variable, MAX_RETURN>::new();
    r.push(s);
    let ret = VariableOperation1::Return(ra, r);
    let all = VariableOperation1::List(vec![func, init, loop_block, ret]);

    // println!("RawOperation: {:#?}", all);
    let scope = VariableOperation2Scope::from(all);

    println!("RawOperationScope: {:#?}", scope);
}
