use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
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
    tmp_reg: Reg,

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
    living_variables: HashMap<Variable, VariableLocation>,
    /// registers ever allocated, used to save register at function calls
    ever_allocated: HashSet<Reg>,
    /// spilled variables for each stack position, len = spill_stack_max
    spill_stack: Box<[Option<Variable>]>,
}

#[derive(Copy, Clone, Hash, Eq, PartialEq, Ord, PartialOrd, Debug)]
enum VariableLocation {
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
            free_regs: Default::default(), //TODO initialize free regs based on reg_usage
            living_variables: Default::default(),
            ever_allocated: Default::default(),
            spill_stack: vec![None; spill_stack_max].into_boxed_slice(),
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
    /// fill variable_info, must be called exactly once before execute()
    pub fn touch(&mut self, op: &VariableOperation3, mut index: usize) -> usize {
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

    //TODO extract program context to save ops and functions?
    //TODO each allocator defines a function? (with real params and return values)
    /// execute
    pub fn execute(
        &mut self,
        op: &VariableOperation3,
        mut index: usize,
        ops: &mut Vec<RegisterOperation>,
    ) -> usize {
        let mut last_result: Option<ResultOp<Reg>> = None;

        index += 1; // TODO &mut usize as param
        match op {
            VariableOperation3::Alloc(v) => {
                self.alloc_for_variable(*v, index, ops);
            }
            VariableOperation3::Result(op) => {
                last_result = Some(match op {
                    ResultOp::Add(r1, r2) => {
                        let r1 = self.prepare_variable(*r1, index, true, ops);
                        let r2 = self.prepare_variable(*r2, index, true, ops);
                        ResultOp::Add(r1, r2)
                    }
                    ResultOp::Addi(r1, i) => {
                        let r1 = self.prepare_variable(*r1, index, true, ops);
                        ResultOp::Addi(r1, *i)
                    }
                    ResultOp::LoadMem(base, offset) => {
                        let base = self.prepare_variable(*base, index, true, ops);
                        ResultOp::LoadMem(base, *offset)
                    }
                });
            }
            VariableOperation3::Update(op) => match op {
                UpdateOp::LoadImmLo(r0, u8) => {
                    let r0 = self.prepare_variable(*r0, index, false, ops);
                    ops.push(RegisterOperation::Update(UpdateOp::LoadImmLo(r0, *u8)));
                }
                UpdateOp::LoadImmHi(r0, u8) => {
                    let r0 = self.prepare_variable(*r0, index, true, ops);
                    ops.push(RegisterOperation::Update(UpdateOp::LoadImmHi(r0, *u8)));
                }
                UpdateOp::Mov(r0, r1) => {
                    let r0 = self.prepare_variable(*r0, index, false, ops);
                    let r1 = self.prepare_variable(*r1, index, true, ops);
                    ops.push(RegisterOperation::Update(UpdateOp::Mov(r0, r1)));
                }
                UpdateOp::AddAssign(r0, r1) => {
                    let r0 = self.prepare_variable(*r0, index, true, ops);
                    let r1 = self.prepare_variable(*r1, index, true, ops);
                    ops.push(RegisterOperation::Update(UpdateOp::AddAssign(r0, r1)));
                }
                UpdateOp::AddiAssign(r0, u4) => {
                    let r0 = self.prepare_variable(*r0, index, true, ops);
                    ops.push(RegisterOperation::Update(UpdateOp::AddiAssign(r0, *u4)));
                }
                UpdateOp::StoreMem(base, offset, r0) => {
                    let r0 = self.prepare_variable(*r0, index, true, ops);
                    let base = self.prepare_variable(*base, index, true, ops);
                    ops.push(RegisterOperation::Update(UpdateOp::StoreMem(
                        base, *offset, r0,
                    )));
                }
            },
            VariableOperation3::Write(v) => {
                assert!(last_result.is_some());
                let op = last_result.take().unwrap();
                let r0 = self.prepare_variable(*v, index, false, ops);
                ops.push(RegisterOperation::Result(op, r0));
            }
            VariableOperation3::Free(v) => {
                let location = self.living_variables.remove(&v).unwrap();
                match location {
                    VariableLocation::Reg(reg) => {
                        self.free_regs
                            .insert(self.reg_usage.reg_info.get(&reg).unwrap().clone());
                    }
                    VariableLocation::Stack(pos) => {
                        assert!(self.spill_stack[pos as usize].is_some());
                        self.spill_stack[pos as usize] = None;
                    }
                }
            }
            VariableOperation3::List(list) => {
                for op in list {
                    index = self.execute(op, index, ops);
                }
            }
            VariableOperation3::If(cond, after_cond, then_block, else_block) => {
                let cond = self.prepare_cond(index, ops, cond);
                if let Some(after_cond) = after_cond {
                    // free operations only
                    index = self.execute(after_cond.as_ref(), index, ops);
                }
                // remember locations of all living variables
                let mut living_prev = self.living_variables.clone();

                // clone current allocator state for else block
                let else_allocator = else_block.as_ref().map(|_| self.clone());

                // process then_block
                let mut then_ops = vec![];
                let index1 = self.execute(then_block.as_ref(), index, &mut then_ops);

                // remove freed variables in target state
                living_prev.retain(|v, _| self.living_variables.contains_key(v));
                // restore locations of all living variables
                self.restore_variable_locations(&living_prev, &mut then_ops);

                let mut else_ops = None;
                let index2 = if let Some(else_block) = else_block {
                    let mut else_allocator = else_allocator.unwrap();
                    else_ops = Some(vec![]);
                    // start from the same index
                    let else_ops = else_ops.as_mut().unwrap();
                    let index2 = else_allocator.execute(else_block, index, else_ops);

                    // restore locations of all living variables
                    else_allocator.restore_variable_locations(&living_prev, else_ops);

                    index2
                } else {
                    index1
                };

                index = index1.max(index2);

                let then_block = RegisterOperation::vec_to_box_ra(then_ops).unwrap();
                let else_block =
                    else_ops.map(|else_ops| RegisterOperation::vec_to_box_ra(else_ops).unwrap());
                ops.push(RegisterOperation::If(cond, then_block, else_block))
            }
            VariableOperation3::Loop(cond, loop_block) => {
                let cond = self.prepare_cond(index, ops, cond);
                let mut loop_ops = vec![];
                index = self.execute(loop_block.as_ref(), index, &mut loop_ops);
                ops.push(RegisterOperation::Loop(
                    cond,
                    RegisterOperation::vec_to_box_ra(loop_ops).unwrap(),
                ));
            }
            VariableOperation3::Func(_name, _return_addr, _params) => {
                //TODO each allocator defines a function???
                // push callee-saved registers (ever used)
                // intiialize param registers
            }
            VariableOperation3::Call(_name, params, return_values) => {
                //TODO execute
                // move variables to param registers
                // push caller-saved registers
                // call
                // pop caller-saved registers
                // intiialize return value registers

                for v in params {
                    self.touch_read(*v, index);
                }
                for v in return_values {
                    self.touch_write(*v, index);
                }
            }
            VariableOperation3::Return(return_addr, return_values) => {
                //TODO execute
                // move variables to return registers
                // pop callee-saved registers
                // return

                self.touch_read(*return_addr, index);
                for v in return_values {
                    self.touch_read(*v, index);
                }
            }
        }

        index
    }

    fn prepare_cond(
        &mut self,
        index: usize,
        ops: &mut Vec<RegisterOperation>,
        cond: &CondOp<Variable>,
    ) -> CondOp<Reg> {
        match cond {
            CondOp::Cmp(r0, r1, c) => {
                let r0 = self.prepare_variable(*r0, index, true, ops);
                let r1 = self.prepare_variable(*r1, index, true, ops);
                CondOp::Cmp(r0, r1, *c)
            }
            CondOp::CmpI(r0, u4, c) => {
                let r0 = self.prepare_variable(*r0, index, true, ops);
                CondOp::CmpI(r0, *u4, *c)
            }
        }
    }
    fn restore_variable_locations(
        &mut self,
        target: &HashMap<Variable, VariableLocation>,
        ops: &mut Vec<RegisterOperation>,
    ) {
        let sp = self.reg_usage.sp_reg;
        let tmp = self.reg_usage.tmp_reg;

        self.living_variables = target.clone();
        self.spill_stack = vec![None; self.reg_usage.spill_stack_max].into_boxed_slice();

        let mapping = target
            .iter()
            .map(|(v, dst)| {
                match dst {
                    VariableLocation::Reg(_) => {}
                    VariableLocation::Stack(pos) => {
                        self.spill_stack[*pos as usize] = Some(*v);
                    }
                }
                let src = self.living_variables.get(v).unwrap();
                (*src, *dst)
            })
            .collect::<BTreeMap<_, _>>();

        let reg_to_reg_ops = move_items(mapping, VariableLocation::Reg(tmp));

        for Move(src, dst) in reg_to_reg_ops {
            match (src, dst) {
                (VariableLocation::Reg(src), VariableLocation::Reg(dst)) => {
                    ops.push(RegisterOperation::Update(UpdateOp::Mov(dst, src)));
                }
                (VariableLocation::Reg(src), VariableLocation::Stack(dst)) => {
                    ops.push(RegisterOperation::Update(UpdateOp::StoreMem(sp, dst, src)));
                }
                (VariableLocation::Stack(src), VariableLocation::Reg(dst)) => {
                    ops.push(RegisterOperation::Result(ResultOp::LoadMem(sp, src), dst));
                }
                (VariableLocation::Stack(src), VariableLocation::Stack(dst)) => {
                    ops.push(RegisterOperation::Result(ResultOp::LoadMem(sp, src), tmp));
                    ops.push(RegisterOperation::Update(UpdateOp::StoreMem(sp, dst, tmp)));
                }
            }
        }
    }

    fn alloc_for_variable(
        &mut self,
        variable: Variable,
        index: usize,
        ops: &mut Vec<RegisterOperation>,
    ) -> Reg {
        // if no free register to use => spill existing
        if self.free_regs.is_empty() {
            let v = self.find_variable_to_spill(index);
            assert_ne!(variable, v);
            self.spill_variable(v, ops);
        }

        // alloc reg
        assert!(!self.free_regs.is_empty());
        let info = self.free_regs.pop_first().unwrap();
        self.living_variables
            .insert(variable, VariableLocation::Reg(info.reg));
        self.ever_allocated.insert(info.reg);
        info.reg
    }
    fn free(&mut self, variable: Variable) {
        let loc = self.living_variables.remove(&variable).unwrap();
        match loc {
            VariableLocation::Reg(reg) => {
                self.free_regs
                    .insert(self.reg_usage.reg_info.get(&reg).unwrap().clone());
            }
            VariableLocation::Stack(pos) => {
                assert_eq!(self.spill_stack[pos as usize], Some(variable));
                self.spill_stack[pos as usize] = None;
            }
        }
    }

    fn prepare_variable(
        &mut self,
        variable: Variable,
        index: usize,
        load_value: bool,
        ops: &mut Vec<RegisterOperation>,
    ) -> Reg {
        let pos = *self.living_variables.get(&variable).unwrap();
        // check variable reg/stack
        match pos {
            // if reg => return reg
            VariableLocation::Reg(reg) => reg,
            // if on stack => alloc reg, read from stack, update allocator
            VariableLocation::Stack(pos) => {
                assert_eq!(self.spill_stack[pos as usize], Some(variable));
                let reg = self.alloc_for_variable(variable, index, ops);
                if load_value {
                    ops.push(RegisterOperation::Result(
                        ResultOp::LoadMem(self.reg_usage.sp_reg, pos),
                        reg,
                    ));
                }
                self.spill_stack[pos as usize] = None;
                // free_regs and living_variables already updated in alloc
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
                VariableLocation::Reg(_) => {
                    let info = self.variable_info.get(v).unwrap();
                    Some((*v, info.distance_to_next_read(curr)))
                }
                VariableLocation::Stack(_) => None,
            })
            // select max distance
            .max_by_key(|(_, dist)| *dist)
            .unwrap();

        println!("found variable to spill: {v:?}, dist {dist:?}");
        v
    }
    fn spill_variable(&mut self, variable: Variable, ops: &mut Vec<RegisterOperation>) {
        // basic checks
        let stat = self.living_variables.get_mut(&variable).cloned();
        assert!(matches!(stat, Some(VariableLocation::Reg(_))));

        if let Some(VariableLocation::Reg(reg)) = stat {
            let info = self.reg_usage.reg_info.get(&reg).unwrap();
            let pos = self.find_empty_stack_pos();

            // now write reg to stack position
            ops.push(RegisterOperation::Update(UpdateOp::StoreMem(
                self.reg_usage.sp_reg,
                pos as u8,
                reg,
            )));

            // update allocator: free reg, set living_variables and spill_stack
            self.free_regs.insert(info.clone());
            self.living_variables
                .insert(variable, VariableLocation::Stack(pos as u8));
            self.spill_stack[pos] = Some(variable);
        }
    }
    fn load_variable_from_stack(
        &mut self,
        variable: Variable,
        stack_pos: usize,
        target_reg: Reg,
        ops: &mut Vec<RegisterOperation>,
    ) {
        let info = self.reg_usage.reg_info.get(&target_reg).unwrap();

        // now write reg to stack position
        ops.push(RegisterOperation::Result(
            ResultOp::LoadMem(self.reg_usage.sp_reg, stack_pos as u8),
            target_reg,
        ));

        // update allocator: free reg, set living_variables and spill_stack
        self.free_regs.remove(info);
        self.living_variables
            .insert(variable, VariableLocation::Reg(target_reg));
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
        call_params: [Reg(2), Reg(3), Reg(4), Reg(5)],
        return_values: [Reg(0), Reg(1)],
        sp_reg: Reg(14),
        tmp_reg: Reg(15),
        spill_stack_max: 16,
    }));

    allocator.touch(&vo3, 0);
    println!("program: {vo3:#?}");
    println!("touch: {:#?}", allocator.variable_info);
}
