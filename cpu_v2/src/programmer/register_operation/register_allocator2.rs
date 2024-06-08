use arrayvec::ArrayVec;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::rc::Rc;

use crate::programmer::*;

#[derive(Clone, Debug)]
pub struct RegisterOperation2(pub RegisterOperation);

/// register allocator with spilling
/// each allocator defines a function
#[derive(Clone)]
pub struct RegisterAllocator2 {
    /// defines calling convention
    reg_usage: Rc<RegisterUsages>,

    /// variable lifetime
    variable_info: HashMap<Variable, VariableTouchInfo>,

    /// callee-saved fake variables, to be restored at return TODO check no param/return reg?
    callee_saved_variables: HashMap<Reg, Variable>,
    /// freed registers, general purpose only
    free_regs: BTreeSet<RegisterInfo>,
    /// living variables
    living_variables: HashMap<Variable, VariableLocation>,
    /// spilled variables for each stack position, len = spill_stack_max
    spill_stack: Box<[Option<Variable>]>,

    // function definition, for code printing and param/return count check
    func_decl: FuncDecl,

    // fields for touch/execute
    /// current touch index to update variable_info
    touch_index: usize,
    /// remember last result op for Write(Variable)
    last_result: Option<ResultOp<Reg>>,
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
    pub fn new(reg_usage: Rc<RegisterUsages>, func_decl: FuncDecl) -> Self {
        let spill_stack_max = reg_usage.spill_stack_max;

        let mut callee_saved_variables = HashMap::new();
        let mut variable_info = HashMap::new();
        let mut living_variables = HashMap::new();

        // all general purpose registers are free
        let free_regs = reg_usage
            .reg_info
            .values()
            .filter(|info| {
                if info.callee_save {
                    let variable = Variable::new();
                    callee_saved_variables.insert(info.reg, variable);
                    living_variables.insert(variable, VariableLocation::Reg(info.reg));
                    variable_info.insert(
                        variable,
                        VariableTouchInfo {
                            reads: vec![],
                            writes: vec![],
                        },
                    );
                    false
                } else {
                    true
                }
            })
            .cloned()
            .collect();

        Self {
            reg_usage,
            variable_info,
            callee_saved_variables,
            free_regs,
            living_variables,
            spill_stack: vec![None; spill_stack_max].into_boxed_slice(),

            func_decl,

            touch_index: 0,
            last_result: None,
        }
    }

    fn touch_write(&mut self, variable: Variable) {
        let entry = self.variable_info.entry(variable).or_default();
        entry.writes.push(self.touch_index);
    }
    fn touch_read(&mut self, variable: Variable) {
        let entry = self.variable_info.entry(variable).or_default();
        entry.reads.push(self.touch_index);
    }
    /// fill variable_info, must be called exactly once before execute()
    pub fn touch(&mut self, op: &VariableOperation3) {
        self.touch_index += 1;
        match op {
            VariableOperation3::Alloc(v) => {
                self.touch_write(*v);
            }
            VariableOperation3::Result(op) => op.touch(|v, ty| match ty {
                TouchType::Input => self.touch_read(*v),
                _ => unreachable!(),
            }),
            VariableOperation3::Update(op) => op.touch(|v, ty| match ty {
                TouchType::Input => self.touch_read(*v),
                _ => unreachable!(),
            }),
            VariableOperation3::Write(v) => {
                self.touch_write(*v);
            }
            VariableOperation3::Free(_v) => {}
            VariableOperation3::List(list) => {
                for op in list {
                    self.touch(op);
                }
            }
            VariableOperation3::If(cond, after_cond, then_block, else_block) => {
                cond.touch(|v, ty| match ty {
                    TouchType::Input => self.touch_read(*v),
                    _ => unreachable!(),
                });
                if let Some(after_cond) = after_cond {
                    self.touch(after_cond.as_ref());
                }

                let start = self.touch_index;
                self.touch(then_block.as_ref());
                let end1 = self.touch_index;

                let end2 = if let Some(else_block) = else_block {
                    // start from the same index
                    self.touch_index = start;
                    self.touch(else_block);
                    self.touch_index
                } else {
                    end1
                };
                self.touch_index = end1.max(end2);
            }
            VariableOperation3::Loop(cond, loop_block) => {
                cond.touch(|v, ty| match ty {
                    TouchType::Input => self.touch_read(*v),
                    _ => unreachable!(),
                });
                self.touch(loop_block.as_ref());
            }
            VariableOperation3::Func(_name, return_addr, params) => {
                for v in params {
                    self.touch_write(*v);
                }
                self.touch_write(*return_addr);
            }
            VariableOperation3::Call(_name, params, return_values) => {
                for v in params {
                    self.touch_read(*v);
                }
                for v in return_values {
                    self.touch_write(*v);
                }
            }
            VariableOperation3::Return(return_addr, return_values) => {
                self.touch_read(*return_addr);
                for v in return_values {
                    self.touch_read(*v);
                }
            }
        }
    }

    /// execute, write register operations
    pub fn execute(&mut self, op: &VariableOperation3, ops: &mut Vec<RegisterOperation>) {
        self.touch_index += 1;
        match op {
            VariableOperation3::Alloc(v) => {
                self.alloc_for_variable(*v, self.touch_index, ops);
            }
            VariableOperation3::Result(op) => {
                self.last_result = Some(match op {
                    ResultOp::Add(r1, r2) => {
                        let r1 = self.prepare_variable(*r1, self.touch_index, true, ops);
                        let r2 = self.prepare_variable(*r2, self.touch_index, true, ops);
                        ResultOp::Add(r1, r2)
                    }
                    ResultOp::Addi(r1, i) => {
                        let r1 = self.prepare_variable(*r1, self.touch_index, true, ops);
                        ResultOp::Addi(r1, *i)
                    }
                    ResultOp::LoadMem(base, offset) => {
                        let base = self.prepare_variable(*base, self.touch_index, true, ops);
                        ResultOp::LoadMem(base, *offset)
                    }
                });
            }
            VariableOperation3::Update(op) => match op {
                UpdateOp::LoadImmLo(r0, u8) => {
                    let r0 = self.prepare_variable(*r0, self.touch_index, false, ops);
                    ops.push(RegisterOperation::Update(UpdateOp::LoadImmLo(r0, *u8)));
                }
                UpdateOp::LoadImmHi(r0, u8) => {
                    let r0 = self.prepare_variable(*r0, self.touch_index, true, ops);
                    ops.push(RegisterOperation::Update(UpdateOp::LoadImmHi(r0, *u8)));
                }
                UpdateOp::Mov(r0, r1) => {
                    let r0 = self.prepare_variable(*r0, self.touch_index, false, ops);
                    let r1 = self.prepare_variable(*r1, self.touch_index, true, ops);
                    ops.push(RegisterOperation::Update(UpdateOp::Mov(r0, r1)));
                }
                UpdateOp::AddAssign(r0, r1) => {
                    let r0 = self.prepare_variable(*r0, self.touch_index, true, ops);
                    let r1 = self.prepare_variable(*r1, self.touch_index, true, ops);
                    ops.push(RegisterOperation::Update(UpdateOp::AddAssign(r0, r1)));
                }
                UpdateOp::AddiAssign(r0, u4) => {
                    let r0 = self.prepare_variable(*r0, self.touch_index, true, ops);
                    ops.push(RegisterOperation::Update(UpdateOp::AddiAssign(r0, *u4)));
                }
                UpdateOp::SubiAssign(r0, u4) => {
                    let r0 = self.prepare_variable(*r0, self.touch_index, true, ops);
                    ops.push(RegisterOperation::Update(UpdateOp::SubiAssign(r0, *u4)));
                }
                UpdateOp::StoreMem(base, offset, r0) => {
                    let r0 = self.prepare_variable(*r0, self.touch_index, true, ops);
                    let base = self.prepare_variable(*base, self.touch_index, true, ops);
                    ops.push(RegisterOperation::Update(UpdateOp::StoreMem(
                        base, *offset, r0,
                    )));
                }
            },
            VariableOperation3::Write(v) => {
                assert!(self.last_result.is_some());
                let op = self.last_result.take().unwrap();
                let r0 = self.prepare_variable(*v, self.touch_index, false, ops);
                ops.push(RegisterOperation::Result(op, r0));
            }
            VariableOperation3::Free(v) => {
                self.free(*v);
            }
            VariableOperation3::List(list) => {
                for op in list {
                    self.execute(op, ops);
                }
            }
            VariableOperation3::If(cond, after_cond, then_block, else_block) => {
                let cond = self.prepare_cond(self.touch_index, ops, cond);
                if let Some(after_cond) = after_cond {
                    // free operations only
                    self.execute(after_cond.as_ref(), ops);
                }
                // remember locations of all living variables
                let mut living_prev = self.living_variables.clone();

                // clone current allocator state for else block
                let else_allocator = else_block.as_ref().map(|_| self.clone());

                // process then_block
                let mut then_ops = vec![];
                self.execute(then_block.as_ref(), &mut then_ops);

                // remove freed variables in target state
                living_prev.retain(|v, _| self.living_variables.contains_key(v));
                // restore locations of all living variables
                self.restore_variable_locations(&living_prev, &mut then_ops);

                let mut else_ops = None;
                let else_touch_index = if let Some(else_block) = else_block {
                    let mut else_allocator = else_allocator.unwrap();
                    else_ops = Some(vec![]);
                    // start from the same index
                    let else_ops = else_ops.as_mut().unwrap();
                    else_allocator.execute(else_block, else_ops);

                    // restore locations of all living variables
                    else_allocator.restore_variable_locations(&living_prev, else_ops);
                    else_allocator.touch_index
                } else {
                    self.touch_index
                };

                self.touch_index = self.touch_index.max(else_touch_index);

                let then_block = RegisterOperation::vec_to_box_ra(then_ops).unwrap();
                let else_block =
                    else_ops.map(|else_ops| RegisterOperation::vec_to_box_ra(else_ops).unwrap());
                ops.push(RegisterOperation::If(cond, then_block, else_block))
            }
            VariableOperation3::Loop(cond, loop_block) => {
                let cond = self.prepare_cond(self.touch_index, ops, cond);
                let mut loop_ops = vec![];
                self.execute(loop_block.as_ref(), &mut loop_ops);
                ops.push(RegisterOperation::Loop(
                    cond,
                    RegisterOperation::vec_to_box_ra(loop_ops).unwrap(),
                ));
            }
            VariableOperation3::Func(_name, return_addr, params) => {
                // this is always the first operation

                // sp -= stack frame size
                ops.push(RegisterOperation::Update(UpdateOp::SubiAssign(
                    self.reg_usage.sp_reg,
                    self.reg_usage.spill_stack_max as u8,
                )));

                // register params to registers
                assert_eq!(params.len(), self.func_decl.param_names.len());
                for (&v, reg) in params.iter().zip(self.reg_usage.params) {
                    self.living_variables.insert(v, VariableLocation::Reg(reg));
                    self.free_regs
                        .remove(&self.reg_usage.reg_info.get(&reg).unwrap().clone());
                }
                // register return_addr to self.reg_usage.return_address
                let return_address_reg = self.reg_usage.return_address;
                self.living_variables
                    .insert(*return_addr, VariableLocation::Reg(return_address_reg));
                self.free_regs.remove(
                    &self
                        .reg_usage
                        .reg_info
                        .get(&return_address_reg)
                        .unwrap()
                        .clone(),
                );
            }
            VariableOperation3::Call(name, params, return_values) => {
                //TODO check func param/return len?

                let param_regs = self.reg_usage.params[0..params.len()]
                    .iter()
                    .cloned()
                    .collect::<ArrayVec<Reg, MAX_PARAM>>();
                let return_regs = self.reg_usage.return_values[0..return_values.len()]
                    .iter()
                    .cloned()
                    .collect::<ArrayVec<Reg, MAX_RETURN>>();

                let mut spill_variables = HashSet::new();

                // call phase
                // 1. find caller-saved variables TODO if it will be freed at call -> can be optimized to a move
                // 2. spill them to stack
                // 3. move params to param registers
                {
                    for (v, pos) in &self.living_variables {
                        if let VariableLocation::Reg(reg) = pos {
                            if self.reg_usage.reg_info.get(reg).unwrap().caller_save {
                                // if variable is caller_saved => spill
                                spill_variables.insert(*v);
                            }
                        }
                    }

                    let usage = self.reg_usage.clone();
                    for v in spill_variables {
                        // ok to spill to register
                        self.spill_variable(v, Some(&usage.caller_save_regs), ops);
                    }

                    for (index, v) in params.iter().enumerate() {
                        match self.living_variables.get(v).unwrap() {
                            VariableLocation::Reg(reg) => {
                                let target = self.reg_usage.params[index];
                                ops.push(RegisterOperation::Update(UpdateOp::Mov(target, *reg)));
                            }
                            VariableLocation::Stack(offset) => {
                                let target = self.reg_usage.params[index];
                                ops.push(RegisterOperation::Result(
                                    ResultOp::LoadMem(self.reg_usage.sp_reg, *offset),
                                    target,
                                ));
                            }
                        }
                    }
                }

                ops.push(RegisterOperation::Call(name, param_regs, return_regs));

                // return phase
                // 1. initialize return value registers
                // 2. leave caller-saved registers as-is
                {
                    for (&v, reg) in return_values.iter().zip(self.reg_usage.return_values) {
                        self.living_variables.insert(v, VariableLocation::Reg(reg));
                        self.free_regs
                            .remove(&self.reg_usage.reg_info.get(&reg).unwrap().clone());
                    }
                }
            }
            VariableOperation3::Return(return_addr, return_values) => {
                assert_eq!(return_values.len(), self.func_decl.return_value_names.len());
                let mut target_regs = HashSet::new();

                // move callee-saved fake variables
                let mut targets: HashMap<Variable, VariableLocation> = HashMap::new();
                for (&reg, &v) in &self.callee_saved_variables {
                    targets.insert(v, VariableLocation::Reg(reg));
                    target_regs.insert(reg);
                }
                // move return values
                let mut return_regs = ArrayVec::<Reg, MAX_RETURN>::new();
                for (&v, reg) in return_values.iter().zip(self.reg_usage.return_values) {
                    targets.insert(v, VariableLocation::Reg(reg));
                    target_regs.insert(reg);
                    return_regs.push(reg);
                }
                // move return addr TODO not necessary to specify a register
                let return_addr_reg = self.reg_usage.return_address;
                targets.insert(*return_addr, VariableLocation::Reg(return_addr_reg));
                target_regs.insert(return_addr_reg);

                println!("current ops: {:?}", ops);

                // restore_variable_locations will destroy self living_variables and spill_stack.
                // we just clone self so that we can support multiple return points in one function and subsequent free ops.
                let mut cloned = self.clone();
                cloned.restore_variable_locations(&targets, ops);

                // sp += stack frame size
                ops.push(RegisterOperation::Update(UpdateOp::AddiAssign(
                    self.reg_usage.sp_reg,
                    self.reg_usage.spill_stack_max as u8,
                )));

                ops.push(RegisterOperation::Return(return_addr_reg, return_regs));
            }
        }
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

        println!("mapping: {:?}", mapping);

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

        self.living_variables = target.clone();
        self.spill_stack = vec![None; self.reg_usage.spill_stack_max].into_boxed_slice();
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
            self.spill_variable(v, None, ops);
        }

        // alloc reg
        assert!(!self.free_regs.is_empty());
        let info = self.free_regs.pop_first().unwrap();
        self.living_variables
            .insert(variable, VariableLocation::Reg(info.reg));
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
    fn spill_variable(
        &mut self,
        variable: Variable,
        allow_to_reg_except: Option<&HashSet<Reg>>,
        ops: &mut Vec<RegisterOperation>,
    ) {
        // basic checks
        let stat = self.living_variables.get_mut(&variable).cloned();
        assert!(matches!(stat, Some(VariableLocation::Reg(_))));

        if let Some(VariableLocation::Reg(reg)) = stat {
            let info = self.reg_usage.reg_info.get(&reg).unwrap();

            let pos = if let Some(exclude) = allow_to_reg_except {
                self.find_empty_location(exclude)
            } else {
                VariableLocation::Stack(self.find_empty_stack_pos())
            };

            match pos {
                VariableLocation::Reg(target_reg) => {
                    // now write reg to stack position
                    ops.push(RegisterOperation::Update(UpdateOp::Mov(target_reg, reg)));
                    self.living_variables
                        .insert(variable, VariableLocation::Reg(target_reg));
                }
                VariableLocation::Stack(pos) => {
                    // now write reg to stack position
                    ops.push(RegisterOperation::Update(UpdateOp::StoreMem(
                        self.reg_usage.sp_reg,
                        pos,
                        reg,
                    )));

                    // update allocator: free reg, set living_variables and spill_stack
                    self.free_regs.insert(info.clone());
                    self.living_variables
                        .insert(variable, VariableLocation::Stack(pos));
                    self.spill_stack[pos as usize] = Some(variable);
                }
            }
        }
    }

    fn find_empty_location(&self, except_reg: &HashSet<Reg>) -> VariableLocation {
        // find in regs
        let valid_reg = self
            .free_regs
            .iter()
            .filter_map(|info| (!except_reg.contains(&info.reg)).then_some(info.reg))
            .next();
        if let Some(valid_reg) = valid_reg {
            return VariableLocation::Reg(valid_reg);
        }

        // find in stack
        self.spill_stack
            .iter()
            .enumerate()
            .find_map(|(i, v)| v.is_none().then_some(VariableLocation::Stack(i as u8)))
            .expect("no empty stack position!")
    }
    fn find_empty_stack_pos(&self) -> u8 {
        self.spill_stack
            .iter()
            .enumerate()
            .find_map(|(i, v)| v.is_none().then_some(i))
            .expect("no empty stack position!") as u8
    }
}

#[cfg(test)]
fn test_program((vo1, decl): (VariableOperation1, FuncDecl)) {
    let vo2s = VariableOperation2Scope::from(vo1);
    let vo3 = VariableOperation3::from(vo2s);
    println!("program: {vo3:#?}");

    let mut allocator = RegisterAllocator2::new(Rc::new(ra2_usages()), decl);
    let mut ops = vec![];

    allocator.touch_index = 0;
    allocator.touch(&vo3);
    println!("touch: {:#?}", allocator.variable_info);

    allocator.touch_index = 0;
    allocator.execute(&vo3, &mut ops);
    println!("execute: {:#?}", ops);
}

#[test]
fn test_basic() {
    test_program(vo1_basic_program());
}
#[test]
fn test_func() {
    test_program(vo1_func_program());
}
#[test]
fn test_if() {
    test_program(vo1_if_program());
}
#[test]
fn test_loop() {
    test_program(vo1_loop_program());
}
#[test]
fn test_spill() {
    test_program(vo1_spill_program());
}
