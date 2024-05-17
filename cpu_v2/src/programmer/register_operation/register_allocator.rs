use crate::programmer::*;
use std::collections::{BinaryHeap, HashMap};

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

type ValidRegsMask = u16;
struct RegisterAllocator {
    /// constant reg priority provided by user
    reg_priority: [usize; 16],
    /// keep track on valid registers
    valid_regs: BinaryHeap<ValidReg>,

    result: Vec<RegisterOperation1>,
    mapping: HashMap<Variable, Reg>,
    last_result: Option<ResultOp<Reg>>,
}
impl RegisterAllocator {
    //TODO configure valid_regs
    pub fn new(priority: [Reg; 16], valid_regs: ValidRegsMask) -> Self {
        let mut this = Self {
            reg_priority: [0; 16],
            valid_regs: BinaryHeap::new(),
            result: vec![],
            mapping: HashMap::default(),
            last_result: None,
        };

        for (i, reg) in priority.into_iter().enumerate() {
            this.reg_priority[reg as usize] = 16 - i;
        }
        for reg in 0..16 {
            if valid_regs & (1 << reg) != 0 {
                this.free(reg);
            }
        }

        this
    }

    fn alloc(&mut self) -> Reg {
        //TODO if empty -> spill, what to return?
        self.valid_regs.pop().unwrap().reg
    }
    fn free(&mut self, reg: Reg) {
        let priority = self.reg_priority[reg as usize];
        self.valid_regs.push(ValidReg { reg, priority });
    }

    pub fn get_valid_regs(&self) -> ValidRegsMask {
        self.valid_regs.iter().map(|v| 1 << v.reg).sum::<u16>()
    }
    pub fn export_ops(&self) -> Vec<RegisterOperation1> {
        self.result.clone()
    }

    pub fn new_op(&mut self, op: VariableOperation3) {
        // match op {
        //     VariableOperation::Alloc(v) => {
        //         let reg = self.alloc();
        //         self.mapping.insert(v, reg);
        //     }
        //     VariableOperation::Result(op) => {
        //         let op2 = op.convert(|v| *self.mapping.get(&v).unwrap());
        //         self.last_result = Some(op2);
        //     }
        //     VariableOperation::Update(op) => {
        //         let op2 = op.convert(|v| *self.mapping.get(&v).unwrap());
        //         self.result
        //             .push(crate::programmer::register_allocator::RegisterOperation::Update(op2));
        //     }
        //     VariableOperation::Write(v) => {
        //         let op = self.last_result.take().unwrap();
        //         let reg = *self.mapping.get(&v).unwrap();
        //         self.result.push(
        //             crate::programmer::register_allocator::RegisterOperation::Result(op, reg),
        //         );
        //     }
        //     VariableOperation::Free(v) => {
        //         let reg = self.mapping.remove(&v).unwrap();
        //         self.free(reg);
        //     }
        // }
    }
}

#[test]
fn test_register_allocator() {
    const PRIORITY: [Reg; 16] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];
    let mut r = RegisterAllocator::new(PRIORITY, u16::MAX);
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
