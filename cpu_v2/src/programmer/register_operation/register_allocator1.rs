use std::collections::{BTreeSet, HashMap, HashSet};

use crate::programmer::*;

#[derive(Clone, Debug)]
pub struct RegisterOperation1(pub RegisterOperation);

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

    fn new_op(&mut self, op: VariableOperation3) -> Vec<RegisterOperation> {
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
                r.push(RegisterOperation::Update(op2));
            }
            VariableOperation3::Write(v) => {
                let op = self.last_result.take().unwrap();
                let reg = *self.mapping.get(&v).unwrap();
                r.push(RegisterOperation::Result(op, reg));
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
                    Box::new(vec_to_ro(else_allocator.new_op(*else_block)))
                });
                let then_op = Box::new(vec_to_ro(self.new_op(*then_block)));

                r.push(RegisterOperation::If(cond, then_op, else_op))
            }
            VariableOperation3::Loop(cond, loop_block) => {
                let cond = cond.convert(|v| *self.mapping.get(&v).unwrap());
                let loop_block = Box::new(vec_to_ro(self.new_op(*loop_block)));
                r.push(RegisterOperation::Loop(cond, loop_block))
            }
            VariableOperation3::Func(func_name, return_addr, params) => {
                let ra = self.alloc(return_addr);
                let params = params.into_iter().map(|v| self.alloc(v)).collect();
                r.push(RegisterOperation::Func(func_name, ra, params))
            }
            VariableOperation3::Call(func_name, params, return_values) => {
                let params = params
                    .into_iter()
                    .map(|v| *self.mapping.get(&v).unwrap())
                    .collect();
                let return_values = return_values.into_iter().map(|v| self.alloc(v)).collect();

                r.push(RegisterOperation::Call(func_name, params, return_values))
            }
            VariableOperation3::Return(return_addr, return_values) => {
                let return_addr = *self.mapping.get(&return_addr).unwrap();
                let return_values = return_values
                    .into_iter()
                    .map(|v| *self.mapping.get(&v).unwrap())
                    .collect();
                r.push(RegisterOperation::Return(return_addr, return_values))
            }
        }

        r
    }

    pub fn process(vo3: VariableOperation3) -> RegisterOperation1 {
        let mut r = RegisterAllocator1::new();
        let vec = r.new_op(vo3);
        RegisterOperation1(vec_to_ro(vec))
    }
}

#[test]
fn test_ra1() {
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

fn vec_to_ro(mut r: Vec<RegisterOperation>) -> RegisterOperation {
    match r.len() {
        0 => panic!("no op?"),
        1 => r.remove(0),
        _ => RegisterOperation::List(r),
    }
}

#[cfg(test)]
fn test_print(vo1: VariableOperation1) {
    let vo2s = VariableOperation2Scope::from(vo1);
    let vo3 = VariableOperation3::from(vo2s);
    let ro1 = RegisterAllocator1::process(vo3);
    println!("ro1: {ro1:#?}");
}
#[test]
fn test_ra1_basic() {
    test_print(vo1_basic_program());
}
#[test]
fn test_ra1_if() {
    test_print(vo1_if_program());
}
#[test]
fn test_ra1_loop() {
    test_print(vo1_loop_program());
}
