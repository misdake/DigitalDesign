use std::collections::HashMap;
use std::fmt::Debug;

use crate::programmer::*;

#[derive(Clone, Debug)]
pub enum RawOperation<T: Oprand> {
    Alloc(T),               // result
    Result(ResultOp<T>, T), // op, result
    Update(UpdateOp<T>),
}
impl<T: Oprand> RawOperation<T> {
    fn touch(&self, mut f: impl FnMut(&T, TouchType)) {
        match self {
            RawOperation::Alloc(v) => f(v, TouchType::UserAlloc),
            RawOperation::Result(op, v) => {
                op.touch(&mut f);
                f(v, TouchType::Output);
            }
            RawOperation::Update(op) => op.touch(&mut f),
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub(crate) enum VariableOperation {
    /// alloc new variable
    Alloc(Variable),
    /// create new variable with operation inputs
    Result(ResultOp<Variable>),
    /// update value of variable
    Update(UpdateOp<Variable>),
    /// write last result op result to this variable
    Write(Variable),
    /// after last result/update usage
    Free(Variable),
}

#[derive(Copy, Clone)]
struct VariableLife {
    alloc: usize,
    first_usage: Option<usize>,
    last_usage: Option<usize>, // keeps updating
}
impl VariableLife {
    fn user_alloc(alloc: usize) -> Self {
        Self {
            alloc,
            first_usage: None,
            last_usage: None,
        }
    }
    fn output(alloc: usize) -> Self {
        Self {
            alloc,
            first_usage: Some(alloc),
            last_usage: None,
        }
    }
    fn need_free(&self, index: usize) -> bool {
        self.last_usage.is_none() || self.last_usage == Some(index)
    }
}
pub struct VariableAllocator {
    ops: Vec<RawOperation<Variable>>,
    variable_life: HashMap<Variable, VariableLife>,
}
impl VariableAllocator {
    pub fn new() -> Self {
        Self {
            ops: vec![],
            variable_life: HashMap::new(),
        }
    }

    pub fn alloc(&mut self) -> Variable {
        let v = Variable::new();
        self.new_op(RawOperation::Alloc(v)); // false to delay alloc till first usage
        v
    }
    pub fn new_result(&mut self, op: ResultOp<Variable>) -> Variable {
        let v = Variable::new();
        self.new_op(RawOperation::Result(op, v));
        v
    }
    pub fn new_update(&mut self, op: UpdateOp<Variable>) {
        self.new_op(RawOperation::Update(op));
    }
    fn new_op(&mut self, op: RawOperation<Variable>) {
        let index = self.ops.len();
        op.touch(|v, t| match t {
            TouchType::Input => {
                let life = self.variable_life.get_mut(v).unwrap();
                if life.first_usage.is_none() {
                    life.first_usage = Some(index);
                }
                life.last_usage = Some(index);
            }
            TouchType::UserAlloc => {
                self.variable_life
                    .insert(*v, VariableLife::user_alloc(index));
            }
            TouchType::Output => {
                self.variable_life.insert(*v, VariableLife::output(index));
            }
        });

        self.ops.push(op);
    }

    pub fn get_cursor(&self) -> usize {
        self.ops.len()
    }

    pub(crate) fn export_ops(&self) -> Vec<VariableOperation> {
        let mut result = vec![];

        for (index, op) in self.ops.iter().cloned().enumerate() {
            match op {
                RawOperation::Alloc(_) => {
                    // not emitting anything
                }
                RawOperation::Result(op, r) => {
                    let raw_op = RawOperation::Result(op, r);

                    self.check_input_alloc(&mut result, index, &raw_op); // solve delayed user alloc
                    result.push(VariableOperation::Result(op));
                    self.check_input_free(&mut result, index, &raw_op); // free last usage

                    result.push(VariableOperation::Alloc(r));
                    result.push(VariableOperation::Write(r));
                    // check output free
                    if self.variable_life.get(&r).unwrap().need_free(index) {
                        result.push(VariableOperation::Free(r));
                    }
                }
                RawOperation::Update(op) => {
                    let raw_op = RawOperation::Update(op);
                    self.check_input_alloc(&mut result, index, &raw_op);
                    result.push(VariableOperation::Update(op));
                    self.check_input_free(&mut result, index, &raw_op);
                }
            }
        }

        result
    }

    fn check_input_alloc(
        &self,
        result: &mut Vec<VariableOperation>,
        index: usize,
        raw_op: &RawOperation<Variable>,
    ) {
        raw_op.touch(|v, t| {
            let life = self.variable_life.get(v).unwrap();
            if t == TouchType::Input && life.first_usage == Some(index) {
                result.push(VariableOperation::Alloc(*v));
            }
        });
    }
    fn check_input_free(
        &self,
        result: &mut Vec<VariableOperation>,
        index: usize,
        raw_op: &RawOperation<Variable>,
    ) {
        raw_op.touch(|v, t| {
            let life = self.variable_life.get(v).unwrap();
            if t == TouchType::Input && life.last_usage == Some(index) {
                result.push(VariableOperation::Free(*v));
            }
        });
    }
}

#[test]
fn test_variable_allocator() {
    let mut r = VariableAllocator::new();
    let a = r.alloc();
    let b = r.alloc();
    r.new_update(UpdateOp::LoadImmLo(a, 1));
    r.new_update(UpdateOp::LoadImmLo(b, 1));
    let c = r.new_result(ResultOp::Add(a, b));
    let d = r.new_result(ResultOp::Add(b, c));
    let _e = r.new_result(ResultOp::Add(c, d));

    let mut reg_count = 0;
    for op in r.export_ops() {
        match op {
            VariableOperation::Alloc(_) => reg_count += 1,
            VariableOperation::Result(_) => {}
            VariableOperation::Update(_) => {}
            VariableOperation::Write(_) => {}
            VariableOperation::Free(_) => reg_count -= 1,
        }
        // no more than 2 registers
        assert!(reg_count <= 2);
        // println!("{op:?}  - reg: {reg_count}");
    }
}
