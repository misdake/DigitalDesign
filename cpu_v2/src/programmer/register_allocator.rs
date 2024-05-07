use crate::isa::Instruction;
use crate::programmer::*;
use std::collections::{BinaryHeap, HashMap};

type Reg = u8; // u4 actually
#[derive(Copy, Clone, Eq, PartialEq)]
struct ValidReg {
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

#[derive(Copy, Clone, Debug)]
enum RegisterOperation {
    Result(ResultOp<Reg>, Reg),
    Update(UpdateOp<Reg>),
}
impl RegisterOperation {
    pub fn to_inst(self) -> Instruction {
        match self {
            RegisterOperation::Result(op, r0) => match op {
                ResultOp::Add(r1, r2) => Instruction::add(r2, r1, r0),
                ResultOp::Addi(r1, i) => {
                    assert!(i >= -8);
                    assert!(i <= 7);
                    Instruction::addi(r1, i as u8, r0)
                }
            },
            RegisterOperation::Update(op) => match op {
                UpdateOp::LoadImmLo(r0, u8) => {
                    let hi = u8 >> 4;
                    let lo = u8 & 0b1111;
                    Instruction::load_lo(hi, lo, r0)
                }
                UpdateOp::LoadImmHi(r0, u8) => {
                    let hi = u8 >> 4;
                    let lo = u8 & 0b1111;
                    Instruction::load_hi(hi, lo, r0)
                }
            },
        }
    }
}

struct RegisterAllocator {
    /// constant reg priority provided by user
    reg_priority: HashMap<Reg, usize>,
    /// keep track on valid registers
    valid_regs: BinaryHeap<ValidReg>,
}
impl RegisterAllocator {
    pub fn new(priority: [Reg; 16]) -> Self {
        let mut reg_priority = HashMap::new();
        let mut valid_regs = BinaryHeap::new();
        for (i, reg) in priority.into_iter().enumerate() {
            reg_priority.insert(reg, 16 - i);
            valid_regs.push(ValidReg {
                reg,
                priority: 16 - i,
            })
        }
        Self {
            reg_priority,
            valid_regs,
        }
    }

    fn alloc(&mut self) -> Reg {
        //TODO if empty -> spill, what to return?
        self.valid_regs.pop().unwrap().reg
    }
    fn free(&mut self, reg: Reg) {
        let priority = *self.reg_priority.get(&reg).unwrap();
        self.valid_regs.push(ValidReg { reg, priority });
    }

    pub fn map_var_to_reg(&mut self, ops: Vec<VariableOperation>) -> Vec<RegisterOperation> {
        let mut result = vec![];
        let mut mapping: HashMap<Variable, Reg> = HashMap::new();
        let mut last_result = None;

        for op in ops {
            match op {
                VariableOperation::Alloc(v) => {
                    let reg = self.alloc();
                    mapping.insert(v, reg);
                }
                VariableOperation::Result(op) => {
                    let op2 = op.convert(|v| *mapping.get(&v).unwrap());
                    last_result = Some(op2);
                }
                VariableOperation::Update(op) => {
                    let op2 = op.convert(|v| *mapping.get(&v).unwrap());
                    result.push(RegisterOperation::Update(op2));
                }
                VariableOperation::Write(v) => {
                    let op = last_result.take().unwrap();
                    let reg = *mapping.get(&v).unwrap();
                    result.push(RegisterOperation::Result(op, reg));
                }
                VariableOperation::Free(v) => {
                    self.free(*mapping.get(&v).unwrap());
                }
            }
        }

        result
    }
}

#[test]
fn test_register_allocator() {
    let mut r = RegisterAllocator::new([0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]);
    let a = r.alloc();
    let b = r.alloc();
    let c = r.alloc();
    assert_eq!(a, 0);
    assert_eq!(b, 1);
    assert_eq!(c, 2);
    r.free(1);
    r.free(2);
    let d = r.alloc();
    let e = r.alloc();
    let f = r.alloc();
    assert_eq!(d, 1);
    assert_eq!(e, 2);
    assert_eq!(f, 3);
}
#[test]
fn test_map_var_to_reg() {
    let mut r = VariableAllocator::new();
    let a = r.alloc();
    let b = r.alloc();
    r.new_update(UpdateOp::LoadImmLo(a, 1));
    r.new_update(UpdateOp::LoadImmLo(b, 1));
    let c = r.new_result(ResultOp::Add(a, b));
    let d = r.new_result(ResultOp::Add(b, c));
    let _e = r.new_result(ResultOp::Add(c, d));

    let ops1 = r.export_ops();
    println!("Variable ops:");
    for op in &ops1 {
        println!("  {op:?}");
    }

    let mut r = RegisterAllocator::new([0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]);

    let ops2 = r.map_var_to_reg(ops1);
    println!("Register ops:");
    for op in &ops2 {
        println!("  {op:?}");
    }

    let instructions = ops2.iter().map(|op| op.to_inst()).collect::<Vec<_>>();
    println!("Instructions:");
    for inst in &instructions {
        println!("  {}", inst);
    }

    use crate::sim::SimEnv;
    let mut env = SimEnv::new(instructions.as_slice());

    let cycles = env.run_to_halt(10);
    assert_eq!(cycles, 5);
    assert_eq!(env.state.reg[0], 5);
}
