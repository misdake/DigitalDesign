use std::collections::HashMap;
use std::hash::Hash;
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

#[derive(Copy, Clone, Debug)]
pub enum RawOperation<T: Oprand> {
    Alloc(T),               // result
    Result(ResultOp<T>, T), // op, result
    Update(UpdateOp<T>),
}
impl<T: Oprand> RawOperation<T> {
    fn op_touch_input(self, mut f: impl FnMut(T)) {
        match self {
            RawOperation::Alloc(_) => {}
            RawOperation::Result(op, _) => match op {
                ResultOp::Add(v1, v2) => {
                    f(v1);
                    f(v2)
                }
                ResultOp::Addi(v, _) => f(v),
            },
            RawOperation::Update(op) => match op {
                UpdateOp::LoadImmLo(v, _) => f(v),
                UpdateOp::LoadImmHi(v, _) => f(v),
            },
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
    pub(crate) fn convert<R: Oprand>(self, mut f: impl FnMut(T) -> R) -> ResultOp<R> {
        match self {
            ResultOp::Add(v1, v2) => ResultOp::Add(f(v1), f(v2)),
            ResultOp::Addi(v, i) => ResultOp::Addi(f(v), i),
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

pub struct VariableAllocator {
    ops: Vec<RawOperation<Variable>>,
    variable_life: HashMap<Variable, (usize, usize)>,
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
        self.new_op(RawOperation::Alloc(v));
        v
    }
    pub fn new_result(&mut self, op: ResultOp<Variable>) -> Variable {
        let v = Variable::new();
        let index = self.ops.len();
        self.new_op(RawOperation::Result(op, v));
        self.variable_life.insert(v, (index, index));
        v
    }
    pub fn new_update(&mut self, op: UpdateOp<Variable>) {
        self.new_op(RawOperation::Update(op));
    }
    fn new_op(&mut self, op: RawOperation<Variable>) {
        let index = self.ops.len();
        self.ops.push(op);
        op.op_touch_input(|v| {
            self.variable_life.entry(v).or_insert((index, index)).1 = index; // first usage also happens here
        });
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
                    self.check_alloc(&mut result, index, raw_op);
                    result.push(VariableOperation::Result(op));
                    self.check_free(&mut result, index, raw_op);

                    if self.variable_life.get(&r).unwrap().0 == index {
                        result.push(VariableOperation::Alloc(r));
                    }
                    result.push(VariableOperation::Write(r));
                    if self.variable_life.get(&r).unwrap().1 == index {
                        result.push(VariableOperation::Free(r));
                    }
                }
                RawOperation::Update(op) => {
                    let raw_op = RawOperation::Update(op);
                    self.check_alloc(&mut result, index, raw_op);
                    result.push(VariableOperation::Update(op));
                    self.check_free(&mut result, index, raw_op);
                }
            }
        }

        result
    }

    fn check_alloc(
        &self,
        mut result: &mut Vec<VariableOperation>,
        index: usize,
        raw_op: RawOperation<Variable>,
    ) {
        raw_op.op_touch_input(|v| {
            if self.variable_life.get(&v).unwrap().0 == index {
                result.push(VariableOperation::Alloc(v));
            }
        });
    }
    fn check_free(
        &self,
        mut result: &mut Vec<VariableOperation>,
        index: usize,
        raw_op: RawOperation<Variable>,
    ) {
        raw_op.op_touch_input(|v| {
            if self.variable_life.get(&v).unwrap().1 == index {
                result.push(VariableOperation::Free(v));
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
