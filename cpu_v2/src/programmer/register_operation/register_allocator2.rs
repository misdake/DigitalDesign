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

    /// freed registers, general purpose only
    free_regs: BTreeSet<RegisterInfo>,
    /// living variables
    living_variables: HashMap<Variable, LivingReg>,
    /// spilled variables for each stack position, len = spill_stack_max
    spill_stack: Box<[Option<Variable>]>,

    ops: Vec<RegisterOperation>,
}

#[derive(Clone, Debug)]
enum LivingReg {
    Reg(Reg),
    Stack(u8), // sp offset
}

#[derive(Clone, Debug, Default)]
struct VariableTouchInfo {
    reads: Vec<usize>,  // sorted
    writes: Vec<usize>, // sorted
}
impl VariableTouchInfo {
    fn distance_to_next_read(&self, curr: usize) -> Option<usize> {
        match self.reads.binary_search(&curr) {
            // found it
            Ok(_) => Some(0),
            Err(index) => {
                if index == self.reads.len() {
                    // end
                    None
                } else {
                    // has next
                    Some(self.reads[index] - curr)
                }
            }
        }
    }
}

impl RegisterAllocator2 {
    pub fn new(reg_usage: Rc<RegisterUsages>) -> Self {
        let spill_stack_max = reg_usage.spill_stack_max;
        Self {
            reg_usage,
            variable_info: Default::default(),
            free_regs: Default::default(),
            living_variables: Default::default(),
            spill_stack: vec![None; spill_stack_max].into_boxed_slice(),
            ops: vec![],
        }
    }

    fn touch_write(&mut self, variable: Variable, index: usize) {
        let entry = self.variable_info.entry(variable).or_default();
        entry.writes.push(index);
    }
    fn touch_read(&mut self, variable: Variable, index: usize) {
        let entry = self.variable_info.entry(variable).or_default();
        entry.reads.push(index);
    }
    /// fill variable_info
    fn touch(&mut self, op: &VariableOperation3, mut index: usize) -> usize {
        index += 1;
        match op {
            VariableOperation3::Alloc(v) => {
                self.touch_write(*v, index);
            }
            VariableOperation3::Result(op) => op.touch(|v, ty| match ty {
                TouchType::Input => self.touch_read(*v, index),
                _ => unreachable!(),
            }),
            VariableOperation3::Update(op) => op.touch(|v, ty| match ty {
                TouchType::Input => self.touch_read(*v, index),
                _ => unreachable!(),
            }),
            VariableOperation3::Write(v) => {
                self.touch_write(*v, index);
            }
            VariableOperation3::Free(_v) => {}
            VariableOperation3::List(list) => {
                for op in list {
                    index = self.touch(op, index);
                }
            }
            VariableOperation3::If(cond, after_cond, then_block, else_block) => {
                cond.touch(|v, ty| match ty {
                    TouchType::Input => self.touch_read(*v, index),
                    _ => unreachable!(),
                });
                if let Some(after_cond) = after_cond {
                    index = self.touch(after_cond.as_ref(), index);
                }
                // start from the same index
                let index1 = self.touch(then_block.as_ref(), index);
                let index2 = if let Some(else_block) = else_block {
                    self.touch(else_block, index)
                } else {
                    index1
                };
                index = index1.max(index2);
            }
            VariableOperation3::Loop(cond, loop_block) => {
                cond.touch(|v, ty| match ty {
                    TouchType::Input => self.touch_read(*v, index),
                    _ => unreachable!(),
                });
                index = self.touch(loop_block.as_ref(), index);
            }
            VariableOperation3::Func(_name, _return_addr, _params) => {}
            VariableOperation3::Call(_name, params, return_values) => {
                for v in params {
                    self.touch_read(*v, index);
                }
                for v in return_values {
                    self.touch_write(*v, index);
                }
            }
            VariableOperation3::Return(return_addr, return_values) => {
                self.touch_read(*return_addr, index);
                for v in return_values {
                    self.touch_read(*v, index);
                }
            }
        }

        index
    }

    fn alloc(&mut self, variable: Variable) -> LivingReg {
        //TODO
        // check free_regs, any => return
        // empty => spill
        todo!()
    }
    fn free(&mut self) {
        todo!()
    }

    fn prepare_variable_as_input(&mut self, variable: Variable) -> Reg {
        let pos = self.living_variables.get(&variable).unwrap().clone();
        // check variable reg/stack
        match pos {
            // if reg => return reg
            LivingReg::Reg(reg) => {
                return reg;
            }
            // if on stack => alloc reg, read from stack, update allocator
            LivingReg::Stack(pos) => {
                let reg = self.alloc();

                reg
            }
        }
    }
    fn find_variable_to_spill(&mut self, curr: usize) -> Variable {
        assert!(self.free_regs.is_empty());

        let (v, dist) = self
            .living_variables
            .iter()
            // get distance to next read of each reg variable
            .filter_map(|(v, state)| match state {
                LivingReg::Reg(_) => {
                    let info = self.variable_info.get(v).unwrap();
                    Some((*v, info.distance_to_next_read(curr)))
                }
                LivingReg::Stack(_) => None,
            })
            // select max distance
            .max_by_key(|(_, dist)| *dist)
            .unwrap();

        println!("found variable to spill: {v:?}, dist {dist:?}");
        v
    }
    fn spill_variable(&mut self, variable: Variable) {
        // basic checks
        let stat = self.living_variables.get_mut(&variable).cloned();
        assert!(matches!(stat, Some(LivingReg::Reg(_))));

        if let Some(LivingReg::Reg(reg)) = stat {
            let info = self.reg_usage.reg_info.get(&reg).unwrap();
            let pos = self.find_empty_stack_pos();

            // now write reg to stack position
            self.ops.push(RegisterOperation::Update(UpdateOp::StoreMem(
                self.reg_usage.sp_reg,
                pos as u8,
                reg,
            )));

            // update allocator: free reg, set living_variables and spill_stack
            self.free_regs.insert(info.clone());
            self.living_variables
                .insert(variable, LivingReg::Stack(pos as u8));
            self.spill_stack[pos] = Some(variable);
        }
    }
    fn load_variable_from_stack(&mut self, variable: Variable, stack_pos: usize, target_reg: Reg) {
        let info = self.reg_usage.reg_info.get(&target_reg).unwrap();

        // now write reg to stack position
        self.ops.push(RegisterOperation::Result(
            ResultOp::LoadMem(self.reg_usage.sp_reg, stack_pos as u8),
            target_reg,
        ));

        // update allocator: free reg, set living_variables and spill_stack
        self.free_regs.remove(info);
        self.living_variables
            .insert(variable, LivingReg::Reg(target_reg));
        self.spill_stack[stack_pos] = None;
    }
    fn find_empty_stack_pos(&self) -> usize {
        self.spill_stack
            .iter()
            .enumerate()
            .find_map(|(i, v)| v.is_none().then_some(i))
            .expect("no empty stack position!")
    }
}

#[test]
fn test_touch() {
    let vo2s = VariableOperation2Scope::from(vo1_basic_program());
    let vo3 = VariableOperation3::from(vo2s);

    let mut allocator = RegisterAllocator2::new(Rc::new(RegisterUsages {
        reg_info: Default::default(),
        call_params: [Reg(0), Reg(1), Reg(2), Reg(3)],
        return_values: [Reg(0), Reg(1)],
        sp_reg: Reg(14),
        temp_reg: Reg(15),
        spill_stack_max: 16,
    }));

    allocator.touch(&vo3, 0);
    println!("program: {vo3:#?}");
    println!("touch: {:#?}", allocator.variable_info);
}
