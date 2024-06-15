use arrayvec::ArrayVec;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::rc::Rc;

use crate::programmer::*;

fn new_op(ops: &mut Vec<RegisterOperation>, op: RegisterOperation) {
    // println!("op: {:?}", op);
    ops.push(op);
}

/// register allocator with spilling
/// each allocator defines a function
#[derive(Clone)]
pub struct RegisterAllocator {
    /// defines calling convention
    reg_usage: Rc<RegisterUsages>,

    /// variable lifetime
    variable_info: HashMap<Variable, VariableTouchInfo>,

    /// callee-saved fake variables, to be restored at return
    callee_saved_variables: HashMap<Reg, Variable>,
    /// ever allocated callee saved registers, reg -> stack offset
    /// when inserting -> save operation will happen at the beginning of function
    allocated_callee_save_regs: BTreeMap<Reg, u8>,
    /// freed registers, general purpose only
    free_regs: BTreeSet<RegisterInfo>,
    /// living variables
    living_variables: HashMap<Variable, VariableLocation>,
    /// spilled variables for each stack position, len = spill_stack_max
    spill_stack: Box<[Option<Variable>]>,
    /// whether to enable stack spilling
    enable_stack: bool,
    /// ever spilled any variable to stack
    stack_used: bool,

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

impl RegisterAllocator {
    pub fn execute(
        reg_usage: Rc<RegisterUsages>,
        func_decl: FuncDecl,
        vo3: &VariableOperation3,
    ) -> Vec<RegisterOperation> {
        let mut ra1 = Self::new(reg_usage, func_decl, true);
        let result = ra1.run(vo3);
        if !ra1.stack_used {
            let mut ra2 = Self::new(ra1.reg_usage, ra1.func_decl, false);
            ra2.run(vo3)
        } else {
            result
        }
    }

    pub fn new(reg_usage: Rc<RegisterUsages>, func_decl: FuncDecl, enable_stack: bool) -> Self {
        let spill_stack_max = reg_usage.spill_stack_max;

        // all general purpose registers are free, callee-saved registers are handled in alloc_variable
        let free_regs = reg_usage.reg_info.values().cloned().collect();

        Self {
            reg_usage,
            variable_info: HashMap::new(),
            callee_saved_variables: HashMap::new(),
            allocated_callee_save_regs: BTreeMap::new(),
            free_regs,
            living_variables: HashMap::new(),
            spill_stack: vec![None; spill_stack_max].into_boxed_slice(),
            enable_stack,
            stack_used: false,

            func_decl,

            touch_index: 0,
            last_result: None,
        }
    }

    pub fn run(&mut self, vo3: &VariableOperation3) -> Vec<RegisterOperation> {
        let mut ops = vec![];

        self.touch_index = 0;
        self.touch(vo3);

        self.touch_index = 0;
        self.execute_vo3(vo3, &mut ops);

        let mut result = vec![];

        // sp -= stack frame size
        if self.enable_stack {
            new_op(
                &mut result,
                RegisterOperation::Update(UpdateOp::SubiAssign(
                    self.reg_usage.sp_reg,
                    self.reg_usage.spill_stack_max as u8,
                )),
            );
        }

        // callee-saved registers
        let sp_reg = self.reg_usage.sp_reg;
        self.allocated_callee_save_regs
            .iter()
            .for_each(|(&reg, &offset)| {
                new_op(
                    &mut result,
                    RegisterOperation::Update(UpdateOp::StoreMem(sp_reg, offset, reg)),
                );
                self.stack_used = true;
            });

        if self.stack_used && !self.enable_stack {
            panic!("stack disabled but used");
        }

        result.extend(ops);
        result
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
    fn touch(&mut self, op: &VariableOperation3) {
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
            VariableOperation3::Call(_name, params, after_params, return_values) => {
                for v in params {
                    self.touch_read(*v);
                }
                if let Some(after_params) = after_params {
                    self.touch(after_params.as_ref());
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
    fn execute_vo3(&mut self, op: &VariableOperation3, ops: &mut Vec<RegisterOperation>) {
        self.touch_index += 1;
        match op {
            VariableOperation3::Alloc(v) => {
                self.alloc_for_variable(*v, self.touch_index, ops);
            }
            VariableOperation3::Result(op) => {
                self.last_result =
                    Some(op.convert(|v| self.prepare_variable(v, self.touch_index, true, ops)));
            }
            VariableOperation3::Update(op) => {
                let op = op.convert(|v, load_value| {
                    self.prepare_variable(v, self.touch_index, load_value, ops)
                });
                new_op(ops, RegisterOperation::Update(op));
            }
            VariableOperation3::Write(v) => {
                assert!(self.last_result.is_some());
                let op = self.last_result.take().unwrap();
                let r0 = self.prepare_variable(*v, self.touch_index, false, ops);
                new_op(ops, RegisterOperation::Result(op, r0));
            }
            VariableOperation3::Free(v) => {
                self.free(*v);
            }
            VariableOperation3::List(list) => {
                for op in list {
                    self.execute_vo3(op, ops);
                }
            }
            VariableOperation3::If(cond, after_cond, then_block, else_block) => {
                let cond = self.prepare_cond(self.touch_index, ops, cond);
                if let Some(after_cond) = after_cond {
                    // free operations only
                    self.execute_vo3(after_cond.as_ref(), ops);
                }
                // remember locations of all living variables
                let mut living_prev = self.living_variables.clone();

                // clone current allocator state for else block
                let else_allocator = else_block.as_ref().map(|_| self.clone());

                // process then_block
                let mut then_ops = vec![];
                self.execute_vo3(then_block.as_ref(), &mut then_ops);

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
                    else_allocator.execute_vo3(else_block, else_ops);

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
                new_op(ops, RegisterOperation::If(cond, then_block, else_block))
            }
            VariableOperation3::Loop(cond, loop_block) => {
                let cond = self.prepare_cond(self.touch_index, ops, cond);
                let mut loop_ops = vec![];
                self.execute_vo3(loop_block.as_ref(), &mut loop_ops);
                new_op(
                    ops,
                    RegisterOperation::Loop(
                        cond,
                        RegisterOperation::vec_to_box_ra(loop_ops).unwrap(),
                    ),
                );
            }
            VariableOperation3::Func(_name, return_addr, params) => {
                // this is always the first operation

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
            VariableOperation3::Call(name, params, after_params, return_values) => {
                // func param/return len to be checked by linker

                let param_regs = self.reg_usage.params[0..params.len()]
                    .iter()
                    .cloned()
                    .collect::<ArrayVec<Reg, MAX_PARAM>>();
                let return_regs = self.reg_usage.return_values[0..return_values.len()]
                    .iter()
                    .cloned()
                    .collect::<ArrayVec<Reg, MAX_RETURN>>();

                let mut freed_params = vec![];
                if let Some(op) = after_params.as_ref() {
                    collect_free(op, &mut freed_params);
                }

                // call phase
                {
                    // 1. find living caller-saved variables
                    let mut spill_variables = HashSet::new();
                    for (v, pos) in &self.living_variables {
                        // if this variable is still living => need to spill to stack or callee save register
                        if !freed_params.contains(v) {
                            if let VariableLocation::Reg(reg) = pos {
                                if self.reg_usage.reg_info.get(reg).unwrap().caller_save {
                                    // if variable is caller_saved => spill
                                    spill_variables.insert(*v);
                                }
                            }
                        }
                    }

                    // 2. spill them to callee save register or stack
                    let usage = self.reg_usage.clone();
                    for v in spill_variables {
                        // ok to spill to register
                        self.spill_variable(v, Some(&usage.caller_save_regs), ops);
                    }

                    // 3. move to-be-freed params to param registers
                    let mut targets = self.living_variables.clone();
                    for (index, v) in params.iter().enumerate() {
                        let target = self.reg_usage.params[index];
                        if freed_params.contains(v) {
                            targets.insert(*v, VariableLocation::Reg(target));
                        }
                    }
                    let mut cloned = self.clone();
                    cloned.restore_variable_locations(&targets, ops);

                    // 4. move spilled params to param registers
                    for (index, v) in params.iter().enumerate() {
                        let target = self.reg_usage.params[index];
                        if !freed_params.contains(v) {
                            match self.living_variables.get(v).unwrap() {
                                VariableLocation::Reg(reg) => {
                                    new_op(
                                        ops,
                                        RegisterOperation::Update(UpdateOp::Mov(target, *reg)),
                                    );
                                }
                                VariableLocation::Stack(offset) => {
                                    new_op(
                                        ops,
                                        RegisterOperation::Result(
                                            ResultOp::LoadMem(self.reg_usage.sp_reg, *offset),
                                            target,
                                        ),
                                    );
                                }
                            }
                        }
                    }
                }

                // free param variables
                if let Some(op) = after_params.as_ref() {
                    self.execute_vo3(op, ops);
                }

                new_op(ops, RegisterOperation::Call(name, param_regs, return_regs));

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
                // move return addr TODO not necessary to specify a register, if it's caller-saved and not return regs => just use it
                let return_addr_reg = self.reg_usage.return_address;
                targets.insert(*return_addr, VariableLocation::Reg(return_addr_reg));
                target_regs.insert(return_addr_reg);

                // restore_variable_locations will destroy self living_variables and spill_stack.
                // we just clone self so that we can support multiple return points in one function and subsequent free ops.
                let mut cloned = self.clone();
                cloned.restore_variable_locations(&targets, ops);

                // restore callee-saved registers, they are not affected by restore_variable_locations
                let sp_reg = self.reg_usage.sp_reg;
                for (&reg, &offset) in &self.allocated_callee_save_regs {
                    new_op(
                        ops,
                        RegisterOperation::Result(ResultOp::LoadMem(sp_reg, offset), reg),
                    );
                }

                // sp += stack frame size
                if self.enable_stack {
                    new_op(
                        ops,
                        RegisterOperation::Update(UpdateOp::AddiAssign(
                            self.reg_usage.sp_reg,
                            self.reg_usage.spill_stack_max as u8,
                        )),
                    );
                }

                new_op(ops, RegisterOperation::Return(return_addr_reg, return_regs));
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

        let reg_to_reg_ops = move_items(mapping, VariableLocation::Reg(tmp));

        for Move(src, dst) in reg_to_reg_ops {
            match (src, dst) {
                (VariableLocation::Reg(src), VariableLocation::Reg(dst)) => {
                    new_op(ops, RegisterOperation::Update(UpdateOp::Mov(dst, src)));
                }
                (VariableLocation::Reg(src), VariableLocation::Stack(dst)) => {
                    new_op(
                        ops,
                        RegisterOperation::Update(UpdateOp::StoreMem(sp, dst, src)),
                    );
                }
                (VariableLocation::Stack(src), VariableLocation::Reg(dst)) => {
                    new_op(
                        ops,
                        RegisterOperation::Result(ResultOp::LoadMem(sp, src), dst),
                    );
                }
                (VariableLocation::Stack(src), VariableLocation::Stack(dst)) => {
                    new_op(
                        ops,
                        RegisterOperation::Result(ResultOp::LoadMem(sp, src), tmp),
                    );
                    new_op(
                        ops,
                        RegisterOperation::Update(UpdateOp::StoreMem(sp, dst, tmp)),
                    );
                }
            }
        }

        self.living_variables = target.clone();
        self.spill_stack = vec![None; self.reg_usage.spill_stack_max].into_boxed_slice();
    }

    fn alloc_variable_to_reg(&mut self, variable: Variable, reg: Reg) {
        let info = self.reg_usage.reg_info.get(&reg).unwrap();
        assert!(self.free_regs.contains(info));
        self.living_variables
            .insert(variable, VariableLocation::Reg(info.reg));
        self.free_regs.remove(info);

        // if alloc callee-saved for the first time
        if info.callee_save && !self.allocated_callee_save_regs.contains_key(&info.reg) {
            let pos = self.find_empty_stack_pos();
            // will be moved to stack at the beginning of function
            self.allocated_callee_save_regs.insert(info.reg, pos);
            self.spill_stack[pos as usize] = Some(Variable::new()); // take this stack position with fake variable
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
            self.spill_variable(v, None, ops);
        }

        // alloc reg
        assert!(!self.free_regs.is_empty());
        let info = self.free_regs.first().unwrap().clone();
        self.alloc_variable_to_reg(variable, info.reg);

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
                    new_op(
                        ops,
                        RegisterOperation::Result(
                            ResultOp::LoadMem(self.reg_usage.sp_reg, pos),
                            reg,
                        ),
                    );
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
                    self.alloc_variable_to_reg(variable, target_reg);
                    let info = self.reg_usage.reg_info.get(&target_reg).unwrap();
                    // now write reg to stack position
                    new_op(
                        ops,
                        RegisterOperation::Update(UpdateOp::Mov(target_reg, reg)),
                    );
                    self.living_variables
                        .insert(variable, VariableLocation::Reg(target_reg));
                    self.free_regs.remove(info);
                }
                VariableLocation::Stack(pos) => {
                    // now write reg to stack position
                    new_op(
                        ops,
                        RegisterOperation::Update(UpdateOp::StoreMem(
                            self.reg_usage.sp_reg,
                            pos,
                            reg,
                        )),
                    );
                    self.stack_used = true;

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

fn collect_free(op: &VariableOperation3, freed: &mut Vec<Variable>) {
    match op {
        VariableOperation3::Free(v) => {
            freed.push(*v);
        }
        VariableOperation3::List(list) => {
            for op in list {
                collect_free(op, freed);
            }
        }
        _ => panic!("unknown op when collecting freed variables"),
    }
}

#[cfg(test)]
fn test_program((vo1, decl): (VariableOperation1, FuncDecl)) {
    let vo2s = VariableOperation2Scope::from(vo1);
    let vo3 = VariableOperation3::from(vo2s);
    println!("program: {vo3:#?}");

    let ops = RegisterAllocator::execute(Rc::new(default_reg_usages()), decl, &vo3);
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
fn test_call() {
    test_program(vo1_call_program(10, 20));
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
fn test_spill1() {
    test_program(vo1_spill_program(20, 1));
}
#[test]
fn test_spill2() {
    test_program(vo1_spill_program(10, 2));
}
