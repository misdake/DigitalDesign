use crate::programmer::*;
use std::collections::{BTreeSet, HashMap, HashSet};

#[derive(Clone, Debug)]
pub struct RegisterOperation2(pub RegisterOperation);

#[derive(Clone, Debug)]
struct RegisterInfo {
    reg: Reg,
    priority: u8,
    caller_save: bool,
    callee_save: bool,
}

// maybe track source register

/// limited register allocator with spilling
#[derive(Clone)]
pub struct RegisterAllocator2 {
    // general purpose registers
    /// general purpose registers with priority
    reg_info: HashMap<Reg, RegisterInfo>,
    /// registers for calling parameters, included in reg_info
    call_params: [Reg; MAX_PARAM],
    /// registers for return values, included in reg_info
    return_values: [Reg; MAX_RETURN],

    // special registers not included in reg_info
    /// stack pointer, base of stack l/s instructions
    sp_reg: Reg,
    /// temporary register, handles imm, return addr, reg swapping
    temp_reg: Reg,

    /// freed registers
    valid_regs: BTreeSet<RegisterInfo>,
    /// living registers and its offset
    living_regs: HashMap<Reg, LivingReg>,
}

#[derive(Clone, Debug)]
struct LivingReg {
    ra1_reg: Reg,
    ra2_reg: Reg,
    sp_offset: u8,
}

impl RegisterAllocator2 {
    fn alloc(&mut self, ra1_reg: Reg) -> Reg {}

    fn process(op: RegisterOperation1) -> RegisterOperation2 {
        match op.0 {
            RegisterOperation::Result(op, r) => {}
            RegisterOperation::Update(op) => {}
            RegisterOperation::List(list) => {}
            RegisterOperation::If(cond, then_block, else_block) => {}
            RegisterOperation::Loop(cond, loop_block) => {}
            RegisterOperation::Func(name, ra, params) => {}
            RegisterOperation::Call(name, params, living_regs, return_values) => {}
            RegisterOperation::Return(ra, return_values, ever_allocated_regs) => {}
        }

        todo!()
    }
}
