use std::cmp::Ordering;
use std::collections::{BTreeSet, HashMap};
use std::rc::Rc;

use crate::programmer::*;

#[derive(Clone, Debug)]
pub struct RegisterOperation2(pub RegisterOperation);

#[derive(Clone, Debug, Eq, PartialEq)]
struct RegisterInfo {
    reg: Reg,
    priority: u8,
    caller_save: bool,
    callee_save: bool,
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

    /// max stack size
    spill_stack_max: usize,
}

/// limited register allocator with spilling
#[derive(Clone)]
pub struct RegisterAllocator2 {
    /// defines calling convention
    reg_usage: Rc<RegisterUsages>,

    /// variable lifetime
    variable_info: HashMap<Variable, VariableTouchInfo>,

    /// freed registers
    free_regs: BTreeSet<RegisterInfo>,
    /// living variables
    living_variables: HashMap<Variable, LivingReg>,
    /// spilled variables for each stack position, len = spill_stack_max
    spill_stack: Box<[Option<Variable>]>,
}

#[derive(Clone, Debug)]
enum LivingReg {
    Reg(Reg),
    Stack(u8), // sp offset
}

#[derive(Clone, Debug)]
struct VariableTouchInfo {
    reads: Vec<usize>,  // sorted
    writes: Vec<usize>, // sorted
}

impl RegisterAllocator2 {
    pub fn new(reg_usage: Rc<RegisterUsages>) -> Self {
        Self {
            reg_usage,
            variable_info: Default::default(),
            free_regs: Default::default(),
            living_variables: Default::default(),
            spill_stack: vec![None; reg_usage.spill_stack_max].into_boxed_slice(),
        }
    }

    /// fill variable_info
    fn touch(&mut self, op: &VariableOperation3) {
        //TODO
        // op.match, touch each variable, fill variable_info
        // for each variable_info, sort
    }

    fn alloc(&mut self, variable: Variable) -> LivingReg {
        //TODO
        // check free_regs, any => return
        // empty => spill
    }
    fn find_and_spill_variable(&mut self) -> LivingReg {
        //TODO
        // for each living variable, find next read
        // select farmost variable to spill
    }

    fn prepare_variable_as_input(&mut self, variable: Variable) -> Reg {
        //TODO
        // check variable reg/stack
        // if reg => return reg
        // if on stack => alloc reg, read from stack
    }
    fn spill_variable(&mut self, variable: Variable) {
        //TODO
        // basic checks
        // find stack position, set living_variables and spill_stack
    }
}
