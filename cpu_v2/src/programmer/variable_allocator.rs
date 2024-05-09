use crate::isa::Cond;
use std::collections::HashMap;
use std::hash::Hash;
use std::slice::SliceIndex;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

static NEXT_VARIABLE: AtomicUsize = AtomicUsize::new(1);

#[derive(Copy, Clone, Hash, Eq, PartialEq, Debug)]
pub struct Variable(usize);
impl Variable {
    fn new() -> Self {
        Self(NEXT_VARIABLE.fetch_add(1, Relaxed))
    }
    fn reset() {
        NEXT_VARIABLE.store(1, Relaxed)
    }
}

pub trait Oprand: Copy + Clone + Hash + Eq + PartialEq {}
impl<T> Oprand for T where T: Copy + Clone + Hash + Eq + PartialEq {}

/// compare .0 with .1
///   for example a > 2 becomes Cmp(a, 2, Cond::Greater)
#[derive(Copy, Clone, Debug)]
pub enum CondOp<T: Oprand> {
    Cmp(T, T, Cond),
    CmpI(T, u16, Cond),
}
impl<T: Oprand> CondOp<T> {
    pub(crate) fn convert<R: Oprand>(self, mut f: impl FnMut(T) -> R) -> CondOp<R> {
        match self {
            CondOp::Cmp(a, b, cond) => CondOp::Cmp(f(a), f(b), cond),
            CondOp::CmpI(a, i, cond) => CondOp::CmpI(f(a), i, cond),
        }
    }
}
impl<T: Oprand> CondOp<T> {
    fn op_touch_input(&self, mut f: impl FnMut(&T, TouchType)) {
        match self {
            CondOp::Cmp(a, b, _) => {
                f(a, TouchType::Input);
                f(b, TouchType::Input);
            }
            CondOp::CmpI(a, _, _) => f(a, TouchType::Input),
        }
    }
}

#[derive(Clone, Debug)]
pub enum RawOperationX<T: Oprand> {
    /// block of operations
    Scope(Vec<RawOperationX<T>>),

    /// result(output)
    Alloc(T),
    /// op, result(output)
    Result(ResultOp<T>, T),
    /// overwrite value, no new results
    Update(UpdateOp<T>),

    /// condition, then, else
    If(
        CondOp<T>,
        Box<RawOperationX<T>>,
        Option<Box<RawOperationX<T>>>,
    ),
    /// condition, loop body
    Loop(CondOp<T>, Box<RawOperationX<T>>), //TODO continue and break? Continue(), Break()

    /// function name, params, return addr(output)
    Call(&'static str, [Option<T>; 4], T),
    /// return addr, return values
    Return(T, [Option<T>; 4]),
}

#[derive(Copy, Clone, Eq, PartialEq)]
enum TouchType {
    Input,
    Output,
    UserAlloc,
}
impl<T: Oprand> RawOperationX<T> {
    fn op_touch_input(&self, mut f: impl FnMut(&T, TouchType)) {
        match self {
            RawOperationX::Scope(list) => {
                for op in list {
                    op.op_touch_input(&mut f);
                }
            }
            RawOperationX::Alloc(v) => f(v, TouchType::Output),
            RawOperationX::Result(op, v) => {
                op.op_touch_input(&mut f);
                f(v, TouchType::Output);
            }
            RawOperationX::Update(op) => op.op_touch_input(&mut f),
            RawOperationX::If(cond, then_block, else_block) => {
                cond.op_touch_input(&mut f);
                then_block.op_touch_input(&mut f);
                if let Some(else_block) = else_block {
                    else_block.op_touch_input(&mut f);
                }
            }
            RawOperationX::Loop(cond, loop_block) => {
                cond.op_touch_input(&mut f);
                loop_block.op_touch_input(&mut f);
            }
            RawOperationX::Call(_, v, addr) => {
                f(addr, TouchType::Output);
                for v in v.iter().flatten() {
                    f(v, TouchType::Input)
                }
            }
            RawOperationX::Return(addr, v) => {
                f(addr, TouchType::Input);
                for v in v.iter().flatten() {
                    f(v, TouchType::Input)
                }
            }
        }
    }
}

#[derive(Clone, Debug)]
pub enum RawOperation<T: Oprand> {
    Alloc(T),               // result
    Result(ResultOp<T>, T), // op, result
    Update(UpdateOp<T>),
}
impl<T: Oprand> RawOperation<T> {
    fn op_touch_input(&self, mut f: impl FnMut(&T, TouchType)) {
        match self {
            RawOperation::Alloc(v) => f(v, TouchType::UserAlloc),
            RawOperation::Result(op, v) => {
                op.op_touch_input(&mut f);
                f(v, TouchType::Output);
            }
            RawOperation::Update(op) => op.op_touch_input(&mut f),
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub enum ResultOp<T: Oprand> {
    Add(T, T),
    Addi(T, i8),
}
#[derive(Copy, Clone, Debug)]
pub enum UpdateOp<T: Oprand> {
    LoadImmLo(T, u8),
    LoadImmHi(T, u8),
}

#[derive(Copy, Clone, Debug)]
pub(crate) enum VariableOperation {
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
}

impl<T: Oprand> ResultOp<T> {
    fn op_touch_input(&self, mut f: impl FnMut(&T, TouchType)) {
        match self {
            ResultOp::Add(v1, v2) => {
                f(v1, TouchType::Input);
                f(v2, TouchType::Input);
            }
            ResultOp::Addi(v, _) => {
                f(v, TouchType::Input);
            }
        }
    }
}
impl<T: Oprand> ResultOp<T> {
    pub(crate) fn convert<R: Oprand>(self, mut f: impl FnMut(T) -> R) -> ResultOp<R> {
        match self {
            ResultOp::Add(v1, v2) => ResultOp::Add(f(v1), f(v2)),
            ResultOp::Addi(v, i) => ResultOp::Addi(f(v), i),
        }
    }
}

impl<T: Oprand> UpdateOp<T> {
    fn op_touch_input(&self, mut f: impl FnMut(&T, TouchType)) {
        match self {
            UpdateOp::LoadImmLo(v, _) => f(v, TouchType::Input),
            UpdateOp::LoadImmHi(v, _) => f(v, TouchType::Input),
        }
    }
}
impl<T: Oprand> UpdateOp<T> {
    pub(crate) fn convert<R: Oprand>(self, mut f: impl FnMut(T) -> R) -> UpdateOp<R> {
        match self {
            UpdateOp::LoadImmLo(v, i) => UpdateOp::LoadImmLo(f(v), i),
            UpdateOp::LoadImmHi(v, i) => UpdateOp::LoadImmHi(f(v), i),
        }
    }
}

pub struct VariableAllocatorX {
    ops: Vec<RawOperation<Variable>>,
    variable_life: HashMap<Variable, (usize, usize)>, // index is op_touch_input order
    curr_index: usize,
}
impl VariableAllocatorX {
    pub fn new() -> Self {
        Self {
            ops: vec![],
            variable_life: HashMap::new(),
            curr_index: 0,
        }
    }
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
        op.op_touch_input(|v, t| match t {
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

    pub(crate) fn export_ops<
        R: SliceIndex<[RawOperation<Variable>], Output = [RawOperation<Variable>]>,
    >(
        &self,
        range: R,
    ) -> Vec<VariableOperation> {
        let mut result = vec![];

        for (index, op) in self.ops[range].iter().cloned().enumerate() {
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
        raw_op.op_touch_input(|v, t| {
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
        raw_op.op_touch_input(|v, t| {
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
    for op in r.export_ops(..) {
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
