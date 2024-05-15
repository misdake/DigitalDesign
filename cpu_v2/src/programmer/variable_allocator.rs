use std::collections::{HashMap, HashSet};
use std::fmt::{Debug, Formatter, Write};
use std::hash::Hash;
use std::ops::Sub;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

use arrayvec::ArrayVec;

use crate::isa::Cond;

const MAX_RETURN: usize = 4;
const MAX_PARAM: usize = 4;
type FuncName = &'static str;

static NEXT_VARIABLE: AtomicUsize = AtomicUsize::new(0);

#[derive(Copy, Clone, Hash, Eq, PartialEq)]
pub struct Variable(usize);
impl Variable {
    fn new() -> Self {
        Self(NEXT_VARIABLE.fetch_add(1, Relaxed))
    }
    fn reset() {
        NEXT_VARIABLE.store(1, Relaxed)
    }
}
impl Debug for Variable {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        const A: u8 = b'a';
        let mut curr = self.0;
        let mut s = String::new();
        if curr == 0 {
            return f.write_char('a');
        }
        while curr > 0 {
            let c = (curr % 26) as u8;
            curr /= 26;
            s.push((A + c) as char);
        }
        f.write_str(&s)
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
    /// overwrite value
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
    /// function name, params(output)
    Func(FuncName, ArrayVec<T, MAX_PARAM>),
    /// function name, params, return addr(output), return values(output)
    Call(FuncName, ArrayVec<T, MAX_PARAM>, T, ArrayVec<T, MAX_RETURN>),
    /// return addr, return values
    Return(T, ArrayVec<T, MAX_RETURN>),
}
#[derive(Clone, Debug)]
pub enum RawOperationExt<T: Oprand> {
    // basic linear operations
    /// result(output)
    Alloc(T),
    /// op, result(output)
    Result(ResultOp<T>, T),
    /// overwrite value
    Update(UpdateOp<T>),

    // recursive structures
    /// list of operations
    List(Vec<RawOperationScope<T>>),
    /// condition, then, else
    If(
        Box<RawOperationScope<T, CondOp<T>>>,
        Box<RawOperationScope<T>>,
        Option<Box<RawOperationScope<T>>>,
    ),
    /// condition, loop body
    Loop(
        Box<RawOperationScope<T, CondOp<T>>>,
        Box<RawOperationScope<T>>,
    ),

    // external flow control
    /// function name, params(output)
    Func(FuncName, ArrayVec<T, MAX_PARAM>),
    /// function name, params, return addr(output), return values(output)
    Call(FuncName, ArrayVec<T, MAX_PARAM>, T, ArrayVec<T, MAX_RETURN>),
    /// return addr, return values
    Return(T, ArrayVec<T, MAX_RETURN>),
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
pub struct ScopeInfo<T: Oprand> {
    // all used inputs
    inputs: HashSet<T>,
    // inputs not in living after
    inputs_drop_after: HashSet<T>,
    // all allocated variables that can be exported (not necessarily used)
    possible_outputs: HashSet<T>,
    // outputs that are used later, subset of all_outputs
    real_outputs: HashSet<T>,
}
impl<T: Oprand> Default for ScopeInfo<T> {
    fn default() -> Self {
        Self {
            inputs: HashSet::new(),
            inputs_drop_after: HashSet::new(),
            possible_outputs: HashSet::new(),
            real_outputs: HashSet::new(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct RawOperationScope<T: Oprand, Op = RawOperationExt<T>> {
    op: Op,
    info: ScopeInfo<T>,
}
impl<T: Oprand> RawOperationScope<T, RawOperationExt<T>> {
    pub fn from(op: RawOperation1<T>) -> Self {
        let mut this = Self::from_raw(op);
        this.second_pass(&mut HashSet::new());
        this
    }
    fn from_raw(op: RawOperation1<T>) -> Self {
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
            RawOperation1::Func(name, param) => Self::func(name, param),
            RawOperation1::Call(name, param, ra, rv) => Self::call(name, param, ra, rv),
            RawOperation1::Return(ra, rv) => Self::ret(ra, rv),
        }
    }

    fn basic(op: RawOperation1<T>) -> Self {
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
            RawOperation1::Alloc(r) => RawOperationExt::Alloc(r),
            RawOperation1::Result(op, r) => RawOperationExt::Result(op, r),
            RawOperation1::Update(op) => RawOperationExt::Update(op),
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
    fn list(list: Vec<RawOperationScope<T>>) -> Self {
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
            op: RawOperationExt::List(list),
            info: ScopeInfo {
                inputs,
                possible_outputs,
                ..Default::default()
            },
        }
    }
    fn if_block(
        cond: CondOp<T>,
        then_block: RawOperationScope<T>,
        else_block: Option<RawOperationScope<T>>,
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

        let cond: RawOperationScope<T, CondOp<T>> = RawOperationScope {
            op: cond,
            info: ScopeInfo {
                inputs: cond_inputs,
                ..Default::default()
            },
        };

        Self {
            op: RawOperationExt::If(
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
    fn loop_block(cond: CondOp<T>, loop_block: RawOperationScope<T>) -> Self {
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

        let cond: RawOperationScope<T, CondOp<T>> = RawOperationScope {
            op: cond,
            info: ScopeInfo {
                inputs: cond_inputs,
                ..Default::default()
            },
        };

        Self {
            op: RawOperationExt::Loop(Box::new(cond), Box::new(loop_block)),
            info: ScopeInfo {
                inputs: loop_inputs,
                ..Default::default()
            },
        }
    }
    fn func(name: &'static str, params: ArrayVec<T, MAX_PARAM>) -> Self {
        let mut possible_outputs = HashSet::new();
        for param in &params {
            possible_outputs.insert(*param);
        }
        // no output

        Self {
            op: RawOperationExt::Func(name, params),
            info: ScopeInfo {
                possible_outputs,
                ..Default::default()
            },
        }
    }
    fn call(
        name: &'static str,
        params: ArrayVec<T, MAX_PARAM>,
        ra: T,
        rv: ArrayVec<T, MAX_RETURN>,
    ) -> Self {
        let mut inputs = HashSet::new();
        let mut possible_outputs = HashSet::new();

        for param in &params {
            inputs.insert(*param);
        }
        possible_outputs.insert(ra);
        for rv in &rv {
            possible_outputs.insert(*rv);
        }

        Self {
            op: RawOperationExt::Call(name, params, ra, rv),
            info: ScopeInfo {
                inputs,
                possible_outputs,
                ..Default::default()
            },
        }
    }
    fn ret(ra: T, rv: ArrayVec<T, MAX_RETURN>) -> Self {
        let mut inputs = HashSet::new();
        for param in &rv {
            inputs.insert(*param);
        }
        inputs.insert(ra);

        Self {
            op: RawOperationExt::Return(ra, rv),
            info: ScopeInfo {
                inputs,
                ..Default::default()
            },
        }
    }
}

impl<T: Oprand> ScopeInfo<T> {
    fn second_pass1(&mut self, later_inputs: &mut HashSet<T>) {
        self.inputs_drop_after = self.inputs.sub(later_inputs);

        self.real_outputs = self
            .possible_outputs
            .drain()
            .filter(|v| later_inputs.contains(v))
            .collect();
    }
    fn second_pass2(&mut self, later_inputs: &mut HashSet<T>) {
        later_inputs.extend(self.inputs.iter().cloned());
        for output in &self.possible_outputs {
            later_inputs.remove(output);
        }
    }
}
impl<T: Oprand> RawOperationScope<T> {
    fn second_pass(&mut self, later_inputs: &mut HashSet<T>) {
        self.info.second_pass1(later_inputs);

        #[allow(clippy::single_match)]
        match &mut self.op {
            RawOperationExt::List(list) => {
                // bottom to top, collect inputs
                for scope in list.iter_mut().rev() {
                    scope.second_pass(later_inputs);
                }
            }
            RawOperationExt::If(cond, then_block, else_block) => {
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
            RawOperationExt::Loop(cond, loop_block) => {
                loop_block.second_pass(later_inputs);
                cond.info.second_pass1(later_inputs);
                cond.info.second_pass2(later_inputs);
            }
            _ => {}
        }

        self.info.second_pass2(later_inputs);
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
        Box::new(RawOperation1::Update(UpdateOp::Mov(d, a))),
        Some(Box::new(RawOperation1::Update(UpdateOp::Mov(d, b)))),
    );
    let result = RawOperation1::Update(UpdateOp::LoadImmHi(d, 1));
    let mut r = ArrayVec::<Variable, MAX_RETURN>::new();
    r.push(d);
    let ret = RawOperation1::Return(d, r);
    let all = RawOperation1::List(vec![init, if_block, result, ret]);

    // println!("RawOperation: {:#?}", all);
    let scope = RawOperationScope::from(all);

    println!("RawOperationScope: {:#?}", scope);
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
    Add(T, T),
    Addi(T, i8),
}
#[derive(Copy, Clone, Debug)]
pub enum UpdateOp<T: Oprand> {
    /// dst, value
    LoadImmLo(T, u8),
    /// dst, value
    LoadImmHi(T, u8),
    /// dst, src
    Mov(T, T),
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
    fn touch(&self, mut f: impl FnMut(&T, TouchType)) {
        match self {
            UpdateOp::LoadImmLo(v, _) => f(v, TouchType::Input),
            UpdateOp::LoadImmHi(v, _) => f(v, TouchType::Input),
            UpdateOp::Mov(dst, src) => {
                f(dst, TouchType::Input);
                f(src, TouchType::Input);
            }
        }
    }
}
impl<T: Oprand> UpdateOp<T> {
    pub(crate) fn convert<R: Oprand>(self, mut f: impl FnMut(T) -> R) -> UpdateOp<R> {
        match self {
            UpdateOp::LoadImmLo(v, i) => UpdateOp::LoadImmLo(f(v), i),
            UpdateOp::LoadImmHi(v, i) => UpdateOp::LoadImmHi(f(v), i),
            UpdateOp::Mov(dst, src) => UpdateOp::Mov(f(dst), f(src)),
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
