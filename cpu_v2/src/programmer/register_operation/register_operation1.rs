use crate::programmer::*;
use arrayvec::ArrayVec;

use std::collections::{BinaryHeap, HashMap};
use std::fmt::{Debug, Formatter};

#[derive(Clone)]
pub enum RegisterOperation1 {
    /// basic ops with output
    Result(ResultOp<Reg>, Reg),
    Update(UpdateOp<Reg>),

    // recursive structures
    /// list of operations
    List(Vec<RegisterOperation1>),
    /// condition, then, else
    If(
        CondOp<Reg>,
        Box<RegisterOperation1>,
        Option<Box<RegisterOperation1>>,
    ),
    /// condition, loop body
    Loop(CondOp<Reg>, Box<RegisterOperation1>),

    // external flow control
    /// function name, return addr(output), params(output)
    Func(FuncName, Reg, ArrayVec<Reg, MAX_PARAM>),
    // /// function name, params, return values(output) TODO implement call with calling convention
    // Call(
    //     FuncName,
    //     ArrayVec<Reg, MAX_PARAM>,
    //     ArrayVec<Reg, MAX_RETURN>,
    // ),
    /// return addr, return values
    Return(Reg, ArrayVec<Reg, MAX_RETURN>),
}
impl Debug for RegisterOperation1 {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            RegisterOperation1::Result(op, r0) => {
                f.write_fmt(format_args!("Result({op:?}, {r0:?})"))
            }
            RegisterOperation1::Update(op) => f.write_fmt(format_args!("Update({op:?})")),
            RegisterOperation1::List(list) => f.debug_list().entries(list).finish(),
            RegisterOperation1::If(cond, t, e) => {
                let mut f = f.debug_struct("If");
                f.field("cond", cond).field("then", t);
                if let Some(e) = e {
                    f.field("else", e);
                }
                f.finish()
            }
            RegisterOperation1::Loop(cond, l) => f
                .debug_struct("Loop")
                .field("cond", cond)
                .field("body", l)
                .finish(),
            RegisterOperation1::Func(name, ra, params) => {
                f.write_fmt(format_args!("Func({name}, {ra:?}, {params:?})"))
            }
            RegisterOperation1::Return(ra, return_values) => {
                f.write_fmt(format_args!("Return({ra:?}, {return_values:?})"))
            }
        }
    }
}

#[derive(Copy, Clone, Eq, PartialEq)]
pub struct ValidReg {
    reg: Reg,
    priority: usize,
}
impl PartialOrd for ValidReg {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.priority.cmp(&other.priority))
    }
}
impl Ord for ValidReg {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.priority.cmp(&other.priority)
    }
}

const MAX_REG_COUNT: usize = 16;
pub type ValidRegsMask = u16;
#[derive(Clone)]
pub struct RegisterAllocator1 {
    /// constant reg priority provided by user
    reg_priority: [usize; MAX_REG_COUNT],
    /// keep track on valid registers
    valid_regs: BinaryHeap<ValidReg>,

    mapping: HashMap<Variable, Reg>,
    last_result: Option<ResultOp<Reg>>,
}
impl RegisterAllocator1 {
    pub fn new(priority: [u8; MAX_REG_COUNT], valid_regs: ValidRegsMask) -> Self {
        let mut this = Self {
            reg_priority: [0; MAX_REG_COUNT],
            valid_regs: BinaryHeap::new(),
            mapping: HashMap::default(),
            last_result: None,
        };

        for (i, reg) in priority.into_iter().enumerate() {
            this.reg_priority[reg as usize] = MAX_REG_COUNT - i;
        }
        for reg in 0..MAX_REG_COUNT {
            if valid_regs & (1 << reg) != 0 {
                this.free(Reg(reg as u8));
            }
        }

        this
    }

    fn alloc(&mut self) -> Reg {
        //TODO if empty -> spill, what to return?
        self.valid_regs.pop().unwrap().reg
    }
    fn free(&mut self, reg: Reg) {
        let priority = self.reg_priority[reg.0 as usize];
        self.valid_regs.push(ValidReg { reg, priority });
    }

    pub fn get_living_variables(&self) -> Vec<Variable> {
        self.mapping.keys().cloned().collect()
    }
    pub fn get_valid_regs(&self) -> ValidRegsMask {
        self.valid_regs.iter().map(|v| 1 << v.reg.0).sum::<u16>()
    }

    pub fn convert(&mut self, op: VariableOperation3) -> RegisterOperation1 {
        let mut r = self.new_op(op);
        match r.len() {
            0 => panic!("no op?"),
            1 => r.remove(0),
            _ => RegisterOperation1::List(r),
        }
    }
    fn new_op(&mut self, op: VariableOperation3) -> Vec<RegisterOperation1> {
        let mut r = vec![];

        fn vec_to_ro1(mut r: Vec<RegisterOperation1>) -> RegisterOperation1 {
            match r.len() {
                0 => panic!("no op?"),
                1 => r.remove(0),
                _ => RegisterOperation1::List(r),
            }
        }

        match op {
            VariableOperation3::Alloc(v) => {
                let reg = self.alloc();
                self.mapping.insert(v, reg);
            }
            VariableOperation3::Result(op) => {
                let op2 = op.convert(|v| *self.mapping.get(&v).unwrap());
                self.last_result = Some(op2);
            }
            VariableOperation3::Update(op) => {
                let op2 = op.convert(|v| *self.mapping.get(&v).unwrap());
                r.push(RegisterOperation1::Update(op2));
            }
            VariableOperation3::Write(v) => {
                let op = self.last_result.take().unwrap();
                let reg = *self.mapping.get(&v).unwrap();
                r.push(RegisterOperation1::Result(op, reg));
            }
            VariableOperation3::Free(v) => {
                let reg = self.mapping.remove(&v).unwrap();
                self.free(reg);
            }
            VariableOperation3::List(list) => {
                for op in list {
                    for op in self.new_op(op) {
                        r.push(op);
                    }
                }
            }
            VariableOperation3::If(cond, free, then_block, else_block) => {
                let cond = cond.convert(|v| *self.mapping.get(&v).unwrap());
                if let Some(b) = free {
                    self.new_op(*b); // free only
                }

                // then and else will free the same variables
                // so for else_block, we clone allocator, convert, then drop this allocator
                let else_op = else_block.map(|else_block| {
                    let mut else_allocator = self.clone();
                    Box::new(vec_to_ro1(else_allocator.new_op(*else_block)))
                });
                let then_op = Box::new(vec_to_ro1(self.new_op(*then_block)));

                r.push(RegisterOperation1::If(cond, then_op, else_op))
            }
            VariableOperation3::Loop(cond, loop_block) => {
                let cond = cond.convert(|v| *self.mapping.get(&v).unwrap());
                let loop_block = Box::new(vec_to_ro1(self.new_op(*loop_block)));
                r.push(RegisterOperation1::Loop(cond, loop_block))
            }
            VariableOperation3::Func(func_name, return_addr, params) => {
                let ra = self.alloc();
                self.mapping.insert(return_addr, ra);
                let params = params
                    .into_iter()
                    .map(|v| {
                        let param = self.alloc();
                        self.mapping.insert(v, param);
                        param
                    })
                    .collect();
                r.push(RegisterOperation1::Func(func_name, ra, params))
            }
            VariableOperation3::Call(func_name, params, return_values) => {
                todo!()
            }
            VariableOperation3::Return(return_addr, return_values) => {
                let return_addr = *self.mapping.get(&return_addr).unwrap();
                let return_values = return_values
                    .into_iter()
                    .map(|v| *self.mapping.get(&v).unwrap())
                    .collect();
                r.push(RegisterOperation1::Return(return_addr, return_values));
            }
        }

        r
    }
}

#[test]
fn test_register_allocator() {
    const PRIORITY: [u8; MAX_REG_COUNT] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];
    let mut r = RegisterAllocator1::new(PRIORITY, 0b0011111111111111); // no 14 and 15
    let a = r.alloc();
    let b = r.alloc();
    let c = r.alloc();
    assert_eq!(a.0, 0);
    assert_eq!(b.0, 1);
    assert_eq!(c.0, 2);
    r.free(b);
    r.free(c);
    let d = r.alloc();
    let e = r.alloc();
    let f = r.alloc();
    assert_eq!(d.0, 1);
    assert_eq!(e.0, 2);
    assert_eq!(f.0, 3);
}

impl RegisterOperation1 {
    pub fn from(vo3: VariableOperation3) -> Self {
        const PRIORITY: [u8; 16] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];
        let mut allocator = RegisterAllocator1::new(PRIORITY, u16::MAX);
        allocator.convert(vo3)
    }
}

#[cfg(test)]
fn test_print(vo1: VariableOperation1) {
    let vo2s = VariableOperation2Scope::from(vo1);
    let vo3 = VariableOperation3::from(vo2s);
    let ro1 = RegisterOperation1::from(vo3);
    println!("ro1: {:#?}", ro1);
}
#[test]
fn test_vo3s_basic() {
    test_print(vo1_basic_program());
}
#[test]
fn test_vo3s_if() {
    test_print(vo1_if_program());
}
#[test]
fn test_vo3s_loop() {
    test_print(vo1_loop_program());
}
