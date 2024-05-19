use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt::{Debug, Formatter};

use arrayvec::ArrayVec;

use crate::programmer::*;

/// basic register operations with unlimited registers
/// calling convention is not considered (handled in later passes)
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
    /// function name, params, living registers, return values(output)
    Call(
        FuncName,
        ArrayVec<Reg, MAX_PARAM>,
        HashSet<Reg>,
        ArrayVec<Reg, MAX_RETURN>,
    ),
    /// return addr, return values, ever allocated registers
    Return(Reg, ArrayVec<Reg, MAX_RETURN>, HashSet<Reg>),
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
            RegisterOperation1::Call(name, params, living_regs, return_values) => f.write_fmt(
                format_args!("Call({name}, {params:?}, {return_values:?}) living: {living_regs:?}"),
            ),
            RegisterOperation1::Return(ra, return_values, ever_allocated_regs) => {
                f.write_fmt(format_args!(
                    "Return({ra:?}, {return_values:?}) ever_allocated: {ever_allocated_regs:?}"
                ))
            }
        }
    }
}

impl RegisterOperation1 {
    pub fn from(vo3: VariableOperation3) -> Self {
        let mut r = RegisterAllocator1::new();
        let vec = r.new_op(vo3);
        vec_to_ro1(vec)
    }

    // //TODO use assembler
    // pub fn into_inst(self) -> Vec<Instruction> {
    //     fn i4_to_u4(i4: i8) -> u8 {
    //         assert!(i4 >= -8);
    //         assert!(i4 <= 7);
    //         (i4 as u8) & 0b1111
    //     }
    //
    //     let mut r = vec![];
    //     match self {
    //         RegisterOperation1::Result(op, r0) => match op {
    //             ResultOp::Add(r1, r2) => r.push(Instruction::add(r2.0, r1.0, r0.0)),
    //             ResultOp::Addi(r1, i4) => r.push(Instruction::addi(r1.0, i4_to_u4(i4), r0.0)),
    //         },
    //         RegisterOperation1::Update(op) => match op {
    //             UpdateOp::Mov(r0, r1) => {
    //                 if r0 != r1 {
    //                     r.push(Instruction::mov(r1.0, r0.0))
    //                 }
    //             }
    //             UpdateOp::LoadImmLo(r0, u8) => {
    //                 let hi = u8 >> 4;
    //                 let lo = u8 & 0b1111;
    //                 r.push(Instruction::load_lo(hi, lo, r0.0))
    //             }
    //             UpdateOp::LoadImmHi(r0, u8) => {
    //                 let hi = u8 >> 4;
    //                 let lo = u8 & 0b1111;
    //                 r.push(Instruction::load_hi(hi, lo, r0.0))
    //             }
    //             UpdateOp::AddAssign(r0, r1) => r.push(Instruction::add(r0.0, r1.0, r0.0)),
    //             UpdateOp::AddiAssign(r0, i4) => r.push(Instruction::addi(r0.0, i4_to_u4(i4), r0.0)),
    //         },
    //
    //         RegisterOperation1::List(list) => {
    //             for op in list {
    //                 r.extend(op.into_inst())
    //             }
    //         }
    //         RegisterOperation1::If(_, _, _) => todo!(),
    //         RegisterOperation1::Loop(_, _) => todo!(),
    //
    //         RegisterOperation1::Func(_, _, _) => {
    //             // todo!()
    //         }
    //         RegisterOperation1::Call(_, _, _) => {
    //             // todo!()
    //         }
    //         RegisterOperation1::Return(_, _) => {
    //             // todo!()
    //         }
    //     }
    //     r
    // }
}

/// unlimited(256) register allocator, first registers are of higher priority
#[derive(Clone)]
pub struct RegisterAllocator1 {
    /// freed registers
    valid_regs: BTreeSet<Reg>,
    /// allocated and not freed registers, used to detect caller saved registers
    living_regs: HashSet<Reg>,
    /// all registered ever used, used to detect callee saved registers
    ever_allocated: HashSet<Reg>,
    /// map variables to registers
    mapping: HashMap<Variable, Reg>,

    /// remember last result op for Write(Variable)
    last_result: Option<ResultOp<Reg>>,
}
impl RegisterAllocator1 {
    pub fn new() -> Self {
        Self {
            valid_regs: BTreeSet::new(),
            living_regs: HashSet::new(),
            ever_allocated: HashSet::new(),
            mapping: HashMap::default(),
            last_result: None,
        }
    }
    fn alloc(&mut self, variable: Variable) -> Reg {
        let reg = if let Some(reg) = self.valid_regs.pop_first() {
            reg
        } else {
            let new_reg = self.ever_allocated.len();
            let reg = Reg(new_reg as u8);
            self.ever_allocated.insert(reg);
            reg
        };
        self.living_regs.insert(reg);
        self.mapping.insert(variable, reg);
        reg
    }
    fn free(&mut self, reg: Reg) {
        self.valid_regs.insert(reg);
        self.living_regs.remove(&reg);
    }

    fn new_op(&mut self, op: VariableOperation3) -> Vec<RegisterOperation1> {
        let mut r = vec![];

        match op {
            VariableOperation3::Alloc(v) => {
                self.alloc(v);
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
                let ra = self.alloc(return_addr);
                let params = params.into_iter().map(|v| self.alloc(v)).collect();
                r.push(RegisterOperation1::Func(func_name, ra, params))
            }
            VariableOperation3::Call(func_name, params, return_values) => {
                let living = self.living_regs.clone();

                let params = params
                    .into_iter()
                    .map(|v| *self.mapping.get(&v).unwrap())
                    .collect();
                let return_values = return_values.into_iter().map(|v| self.alloc(v)).collect();

                r.push(RegisterOperation1::Call(
                    func_name,
                    params,
                    living,
                    return_values,
                ))
            }
            VariableOperation3::Return(return_addr, return_values) => {
                let return_addr = *self.mapping.get(&return_addr).unwrap();
                let return_values = return_values
                    .into_iter()
                    .map(|v| *self.mapping.get(&v).unwrap())
                    .collect();
                let ever_allocated = self.ever_allocated.clone();
                r.push(RegisterOperation1::Return(
                    return_addr,
                    return_values,
                    ever_allocated,
                ))
            }
        }

        r
    }
}

#[test]
fn test_ro1() {
    let mut r = RegisterAllocator1::new();
    let a = r.alloc(Variable::new());
    let b = r.alloc(Variable::new());
    let c = r.alloc(Variable::new());
    assert_eq!(a.0, 0);
    assert_eq!(b.0, 1);
    assert_eq!(c.0, 2);
    r.free(b);
    r.free(c);
    let d = r.alloc(Variable::new());
    let e = r.alloc(Variable::new());
    let f = r.alloc(Variable::new());
    assert_eq!(d.0, 1);
    assert_eq!(e.0, 2);
    assert_eq!(f.0, 3);
}

fn vec_to_ro1(mut r: Vec<RegisterOperation1>) -> RegisterOperation1 {
    match r.len() {
        0 => panic!("no op?"),
        1 => r.remove(0),
        _ => RegisterOperation1::List(r),
    }
}

#[cfg(test)]
fn test_print(vo1: VariableOperation1) {
    let vo2s = VariableOperation2Scope::from(vo1);
    let vo3 = VariableOperation3::from(vo2s);
    let ro1 = RegisterOperation1::from(vo3);
    println!("ro1: {ro1:#?}");
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
