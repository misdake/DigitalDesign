use crate::programmer::{Reg, MAX_PARAM, MAX_RETURN};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

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

    pub caller_save_regs: HashSet<Reg>,
    pub callee_save_regs: HashSet<Reg>,

    // special registers not included in reg_info
    /// stack pointer, base of stack l/s instructions
    pub sp_reg: Reg,
    /// temporary register, handles imm, return addr, reg swapping
    pub tmp_reg: Reg,

    /// max stack size
    pub spill_stack_max: usize,
}

pub const RETURN_ADDR_REG: u8 = 13;
pub use crate::isa::SP_REG;
pub const TMP_REG: u8 = 15;

/// default register usages:
/// return 0, 1, param 2, 3, 4, 5, return addr 13
/// caller save: 0, 1, 2, 3, 4, 5, 6, 13
/// callee save: 7, 8, 9, 10, 11, 12
pub fn default_reg_usages() -> RegisterUsages {
    let caller_save = vec![0, 1, 2, 3, 4, 5, 6, 13];
    let callee_save = vec![7, 8, 9, 10, 11, 12];

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
        caller_save_regs: caller_save.into_iter().map(Reg).collect(),
        callee_save_regs: callee_save.into_iter().map(Reg).collect(),
        params: [Reg(2), Reg(3), Reg(4), Reg(5)],
        return_address: Reg(RETURN_ADDR_REG),
        return_values: [Reg(0), Reg(1)],
        sp_reg: Reg(SP_REG),
        tmp_reg: Reg(TMP_REG),
        // u8 offset can address 256 slots, but sp_sub takes a u8 (max 255), so the reserved frame is capped at 255 slots
        spill_stack_max: 255,
    }
}
