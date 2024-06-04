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
    params: [Reg; MAX_PARAM],
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

/// register allocator with spilling
/// each allocator defines a function
#[derive(Clone)]
pub struct RegisterAllocator2 {
    /// defines calling convention
    reg_usage: Rc<RegisterUsages>,

    /// variable lifetime
    variable_info: HashMap<Variable, VariableTouchInfo>,

    /// callee-saved fake variables, to be restored at return
    callee_saved_variables: HashMap<Reg, Variable>,
    /// freed registers, general purpose only
    free_regs: BTreeSet<RegisterInfo>,
    /// living variables
    living_variables: HashMap<Variable, VariableLocation>,
    /// registers ever allocated, used to save register at function calls
    ever_allocated: HashSet<Reg>,
    /// spilled variables for each stack position, len = spill_stack_max
    spill_stack: Box<[Option<Variable>]>,

    // function definition, for code printing and param/return count check
    func_name: FuncName,
    params: Vec<&'static str>,
    return_values_names: Vec<&'static str>,

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

//TODO replace ctx with self
pub struct ExecuteContext<'a> {
    index: &'a mut usize,
    last_result: &'a mut Option<ResultOp<Reg>>,
}

impl RegisterAllocator2 {
    pub fn new(
        reg_usage: Rc<RegisterUsages>,
        func_name: FuncName,
        params: &[&'static str],
        return_value_names: &[&'static str],
    ) -> Self {
        let spill_stack_max = reg_usage.spill_stack_max;

        let mut callee_saved_variables = HashMap::new();
        let mut variable_info = HashMap::new();
        let mut living_variables = HashMap::new();
        let mut ever_allocated = HashSet::new();

        let mut callee_save_variables: Vec<Variable> = vec![];

        // all general purpose registers are free
        let free_regs = reg_usage
            .reg_info
            .values()
            .filter(|info| {
                if info.callee_save {
                    let variable = Variable::new();
                    callee_saved_variables.insert(info.reg, variable);
                    callee_save_variables.push(variable);
                    living_variables.insert(variable, VariableLocation::Reg(info.reg));
                    ever_allocated.insert(info.reg);
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
            ever_allocated,
            spill_stack: vec![None; spill_stack_max].into_boxed_slice(),

            func_name,
            params: Vec::from(params),
            return_values_names: Vec::from(return_value_names),

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
            VariableOperation3::Func(_name, _return_addr, _params) => {
                //TODO assert param count
                // intiialize param registers
            }
            VariableOperation3::Call(_name, params, return_values) => {
                //TODO execute
                // move variables to param registers
                // push caller-saved registers
                // call
                // pop caller-saved registers
                // intiialize return value registers
            }
            VariableOperation3::Return(return_addr, return_values) => {
                //TODO execute
                // assert return count
                // move variables to return registers, including callee-saved fake variables
                // return
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
        params: [Reg(2), Reg(3), Reg(4), Reg(5)],
        return_values: [Reg(0), Reg(1)],
        sp_reg: Reg(14),
        tmp_reg: Reg(15),
        spill_stack_max: 16,
    }));

    let mut index = 0;
    let mut last_result: Option<ResultOp<Reg>> = None;
    let mut ctx = ExecuteContext {
        index: &mut index,
        last_result: &mut last_result,
    };
    allocator.touch(&vo3, &mut ctx);
    println!("program: {vo3:#?}");
    println!("touch: {:#?}", allocator.variable_info);
}
