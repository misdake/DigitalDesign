use crate::programmer::{Reg, MAX_PARAM, MAX_RETURN};
use std::cmp::Ordering;
use std::collections::HashMap;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegisterInfo {
    pub reg: Reg,
    pub priority: u8,
    pub caller_save: bool,
    pub callee_save: bool,
}

impl PartialOrd for RegisterInfo {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.priority.partial_cmp(&other.priority)
    }
}
impl Ord for RegisterInfo {
    fn cmp(&self, other: &Self) -> Ordering {
        self.priority.cmp(&other.priority)
    }
}

// maybe track source register

#[derive(Clone, Debug)]
pub struct RegisterUsages {
    // general purpose registers
    /// general purpose registers with priority
    pub reg_info: HashMap<Reg, RegisterInfo>,
    /// registers for calling parameters, included in reg_info
    pub params: [Reg; MAX_PARAM],
    /// registers for return address, included in reg_info, specified only between call<>func
    pub return_address: Reg,
    /// registers for return values, included in reg_info
    pub return_values: [Reg; MAX_RETURN],

    // special registers not included in reg_info
    /// stack pointer, base of stack l/s instructions
    pub sp_reg: Reg,
    /// temporary register, handles imm, return addr, reg swapping
    pub tmp_reg: Reg,

    /// max stack size
    pub spill_stack_max: usize,
}

/// default register usages:
///
pub fn ra2_usages() -> RegisterUsages {
    let caller_save = vec![0, 1, 2, 3, 4, 5, 6, 7];
    let callee_save = vec![8, 9, 10, 11, 12]; // 13 is return address

    let order = vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13];

    let mut reg_info: HashMap<Reg, RegisterInfo> = HashMap::new();
    for i in 0..14 {
        reg_info.insert(
            Reg(i),
            RegisterInfo {
                reg: Reg(i),
                priority: order.iter().position(|r| *r == i).unwrap() as u8,
                caller_save: caller_save.contains(&i),
                callee_save: callee_save.contains(&i),
            },
        );
    }

    RegisterUsages {
        reg_info,
        params: [Reg(2), Reg(3), Reg(4), Reg(5)],
        return_address: Reg(13),
        return_values: [Reg(0), Reg(1)],
        sp_reg: Reg(14),
        tmp_reg: Reg(15),
        spill_stack_max: 15,
    }
}
