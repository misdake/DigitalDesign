use arrayvec::ArrayVec;
use std::collections::HashMap;
use std::hash::Hash;
use std::ops::Deref;
use std::slice::SliceIndex;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

use smallvec::SmallVec;

use crate::isa::Cond;

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
    fn touch(&self, mut f: impl FnMut(&T, TouchType)) {
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
pub enum RawOperation1<T: Oprand> {
    // basic linear operations
    /// result(output)
    Alloc(T),
    /// op, result(output)
    Result(ResultOp<T>, T),
    /// overwrite value, no new results
    Update(UpdateOp<T>),

    // recursive structures
    /// list of operations
    List(Vec<RawOperation1<T>>),
    /// condition, then, else
    If(
        CondOp<T>,
        Box<RawOperation1<T>>,
        Option<Box<RawOperation1<T>>>,
    ),
    /// condition, loop body
    Loop(CondOp<T>, Box<RawOperation1<T>>), //TODO support continue and break

    // external flow control
    /// function name, params, return addr(output)
    Call(&'static str, ArrayVec<T, 4>, T),
    /// return addr, return values
    Return(T, ArrayVec<T, 4>),
}
#[derive(Clone, Debug)]
pub enum RawOperationExt<T: Oprand> {
    // basic linear operations
    /// result(output)
    Alloc(T),
    /// op, result(output)
    Result(ResultOp<T>, T),
    /// overwrite value, no new results
    Update(UpdateOp<T>),

    // recursive structures
    /// list of operations
    List(Vec<RawOperationScope<T>>),
    /// condition, then, else
    If(
        CondOp<T>,
        Box<RawOperationScope<T>>,
        Option<Box<RawOperationScope<T>>>,
    ),
    /// condition, loop body
    Loop(CondOp<T>, Box<RawOperationScope<T>>),

    // external flow control
    /// function name, params, return addr(output)
    Call(&'static str, ArrayVec<T, 4>, T),
    /// return addr, return values
    Return(T, ArrayVec<T, 4>),
}
impl<T: Oprand> RawOperation1<T> {
    fn touch_primitive(&self, mut f: impl FnMut(&T, TouchType)) {
        match self {
            RawOperation1::Alloc(v) => f(v, TouchType::UserAlloc),
            RawOperation1::Result(op, v) => {
                op.touch(&mut f);
                f(v, TouchType::Output);
            }
            RawOperation1::Update(op) => op.touch(&mut f),
            _ => {}
        }
    }
}

#[derive(Clone, Debug)]
pub struct RawOperationScope<T: Oprand> {
    op: RawOperationExt<T>,
    // internal variables allocated in this scope
    variable_alloc_in_scope: SmallVec<[T; 1]>,
    // all used inputs
    used_inputs: SmallVec<[T; 2]>,
    // all outputs that can be exported (not necessarily used)
    all_outputs: SmallVec<[T; 1]>,
}
impl<T: Oprand> Deref for RawOperationScope<T> {
    type Target = RawOperationExt<T>;
    fn deref(&self) -> &Self::Target {
        &self.op
    }
}
impl<T: Oprand> RawOperationScope<T> {
    pub fn from_raw(op: RawOperation1<T>) -> Self {
        match op {
            op @ RawOperation1::Alloc(_)
            | op @ RawOperation1::Result(_, _)
            | op @ RawOperation1::Update(_) => Self::basic(op),
            RawOperation1::List(list) => {
                let list = list
                    .into_iter()
                    .map(|item| Self::from_raw(item))
                    .collect::<Vec<_>>();
                Self::list(list)
            }
            RawOperation1::If(cond, if_block, else_block) => {
                let if_block = Self::from_raw(*if_block);
                let else_block = else_block.map(|op| Self::from_raw(*op));
                Self::if_block(cond, if_block, else_block)
            }
            RawOperation1::Loop(cond, loop_block) => {
                let loop_block = Self::from_raw(*loop_block);
                Self::loop_block(cond, loop_block)
            }
            RawOperation1::Call(name, param, ra) => Self::call(name, param, ra),
            RawOperation1::Return(ra, rv) => Self::ret(ra, rv),
        }
    }

    fn basic(op: RawOperation1<T>) -> Self {
        let mut variable_alloc_in_scope = SmallVec::new();
        let mut used_inputs = SmallVec::new();
        let mut all_outputs = SmallVec::new();

        // touch top level
        op.touch_primitive(|v, ty| match ty {
            TouchType::Input => {
                if !variable_alloc_in_scope.contains(v) {
                    used_inputs.push(*v);
                }
            }
            TouchType::Output => {
                variable_alloc_in_scope.push(*v);
                all_outputs.push(*v);
            }
            TouchType::UserAlloc => {
                variable_alloc_in_scope.push(*v);
                all_outputs.push(*v);
            }
        });

        let op = match op {
            RawOperation1::Alloc(r) => RawOperationExt::Alloc(r),
            RawOperation1::Result(op, r) => RawOperationExt::Result(op, r),
            RawOperation1::Update(op) => RawOperationExt::Update(op),
            _ => {
                unreachable!()
            }
        };

        Self {
            variable_alloc_in_scope,
            op,
            used_inputs,
            all_outputs,
        }
    }
    fn list(list: Vec<RawOperationScope<T>>) -> Self {
        let mut variable_alloc_in_scope = SmallVec::new();
        let mut used_inputs = SmallVec::new();
        let mut all_outputs = SmallVec::new();

        for op in &list {
            variable_alloc_in_scope.extend_from_slice(&op.variable_alloc_in_scope);
            for v in &op.used_inputs {
                if !variable_alloc_in_scope.contains(v) {
                    used_inputs.push(*v); // list have all external inputs
                }
            }
            all_outputs.extend_from_slice(&op.all_outputs); // list have all outputs
        }

        let op = RawOperationExt::List(list);

        Self {
            variable_alloc_in_scope,
            op,
            used_inputs,
            all_outputs,
        }
    }
    fn if_block(
        cond: CondOp<T>,
        then_block: RawOperationScope<T>,
        else_block: Option<RawOperationScope<T>>,
    ) -> Self {
        // inputs from cond and two blocks
        let mut used_inputs = SmallVec::new();
        cond.touch(|v, ty| match ty {
            TouchType::Input => {
                used_inputs.push(*v);
            }
            _ => {
                unreachable!("no alloc or output allowed in CondOp")
            }
        });
        used_inputs.extend_from_slice(&then_block.used_inputs);
        if let Some(else_block) = &else_block {
            used_inputs.extend_from_slice(&else_block.used_inputs);
        }
        // no output

        let op = RawOperationExt::If(cond, Box::new(then_block), else_block.map(Box::new));

        Self {
            variable_alloc_in_scope: SmallVec::new(),
            op,
            used_inputs,
            all_outputs: SmallVec::new(),
        }
    }
    fn loop_block(cond: CondOp<T>, loop_block: RawOperationScope<T>) -> Self {
        // inputs from cond and loop block
        let mut used_inputs = SmallVec::new();
        cond.touch(|v, ty| match ty {
            TouchType::Input => {
                used_inputs.push(*v);
            }
            _ => {
                unreachable!("no alloc or output allowed in CondOp")
            }
        });
        used_inputs.extend_from_slice(&loop_block.used_inputs);
        // no output

        let op = RawOperationExt::Loop(cond, Box::new(loop_block));

        Self {
            variable_alloc_in_scope: SmallVec::new(),
            op,
            used_inputs,
            all_outputs: SmallVec::new(),
        }
    }
    fn call(name: &'static str, params: ArrayVec<T, 4>, ra: T) -> Self {
        let mut variable_alloc_in_scope = SmallVec::new();
        let mut used_inputs = SmallVec::new();
        let mut all_outputs = SmallVec::new();

        for param in &params {
            used_inputs.push(*param)
        }
        variable_alloc_in_scope.push(ra);
        all_outputs.push(ra);

        Self {
            op: RawOperationExt::Call(name, params, ra),
            variable_alloc_in_scope,
            used_inputs,
            all_outputs,
        }
    }
    fn ret(ra: T, rv: ArrayVec<T, 4>) -> Self {
        let mut used_inputs = SmallVec::new();

        for param in &rv {
            used_inputs.push(*param)
        }
        used_inputs.push(ra);

        Self {
            op: RawOperationExt::Return(ra, rv),
            variable_alloc_in_scope: SmallVec::new(),
            used_inputs,
            all_outputs: SmallVec::new(),
        }
    }
}

#[test]
fn test_raw_operation_scope() {
    let a = Variable::new();
    let b = Variable::new();
    let c = Variable::new();
    let d = Variable::new();

    let init = RawOperation1::List(vec![
        RawOperation1::Alloc(a),
        RawOperation1::Alloc(b),
        RawOperation1::Alloc(c),
        RawOperation1::Alloc(d),
        RawOperation1::Update(UpdateOp::LoadImmLo(a, 10)),
        RawOperation1::Update(UpdateOp::LoadImmLo(b, 20)),
        RawOperation1::Update(UpdateOp::LoadImmLo(c, 2)),
        RawOperation1::Update(UpdateOp::LoadImmLo(d, 30)),
    ]);
    let if_block = RawOperation1::If(
        CondOp::CmpI(c, 1, Cond::Greater),
        Box::new(RawOperation1::Result(ResultOp::Mov(a), d)),
        Some(Box::new(RawOperation1::Result(ResultOp::Mov(b), d))),
    );
    let result = RawOperation1::Update(UpdateOp::LoadImmHi(d, 1));
    let all = RawOperation1::List(vec![init, if_block, result]);

    println!("RawOperation: {:?}", all);
    println!("RawOperationScope: {:?}", RawOperationScope::from_raw(all));
}

#[derive(Copy, Clone, Eq, PartialEq)]
enum TouchType {
    Input,
    Output,
    UserAlloc,
}

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
pub enum ResultOp<T: Oprand> {
    Mov(T),
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
    fn touch(&self, mut f: impl FnMut(&T, TouchType)) {
        match self {
            ResultOp::Mov(v) => {
                f(v, TouchType::Input);
            }
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
            ResultOp::Mov(v) => ResultOp::Mov(f(v)),
            ResultOp::Add(v1, v2) => ResultOp::Add(f(v1), f(v2)),
            ResultOp::Addi(v, i) => ResultOp::Addi(f(v), i),
        }
    }
}

impl<T: Oprand> UpdateOp<T> {
    fn touch(&self, mut f: impl FnMut(&T, TouchType)) {
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
    variable_life: HashMap<Variable, (usize, usize)>, // index is touch order
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
