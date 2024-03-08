use crate::isa::Instruction;
use digital_design_code::select;

pub struct SimEnv {
    inst: Box<[Instruction; 65536]>,
    state: SimState,
}

struct SimState {
    reg: [u16; 16],
    mem: Box<[u16; 65536]>,
    pc: u16,
    stack_ptr: u16,
    flags: u8,
}

impl Default for SimState {
    fn default() -> Self {
        Self {
            reg: [0; 16],
            mem: Box::new([0; 65536]),
            pc: 0,
            stack_ptr: 0,
            flags: 0,
        }
    }
}

#[derive(Copy, Clone, Debug, Ord, PartialOrd, Eq, PartialEq)]
pub enum StateChange {
    Reg(u8, u16),
    Mem(u16, u16),
    Pc(u16),
    StackPtr(u16),
    Flags(u8),
}

impl SimEnv {
    pub fn new(inst: Box<[Instruction; 65536]>) -> SimEnv {
        Self {
            inst,
            state: SimState::default(),
        }
    }

    pub fn eval(&self) -> Vec<StateChange> {
        let mut changes = vec![];

        let pc = self.state.pc;
        let inst = self.inst[pc as usize];
        let mut pc_next = pc + 1;
        let reg = |r: u8| self.state.reg[r as usize];
        let mem = |addr: u16| self.state.mem[addr as usize];

        // real sim
        match inst {
            Instruction::and(r2, r1, r0) => changes.push(StateChange::Reg(r0, reg(r1) & reg(r2))),
            Instruction::or(r2, r1, r0) => changes.push(StateChange::Reg(r0, reg(r1) | reg(r2))),
            Instruction::xor(r2, r1, r0) => changes.push(StateChange::Reg(r0, reg(r1) ^ reg(r2))),
            Instruction::add(r2, r1, r0) => {
                changes.push(StateChange::Reg(r0, reg(r1).wrapping_add(reg(r2))))
            }
            Instruction::sub(r2, r1, r0) => {
                changes.push(StateChange::Reg(r0, reg(r1).wrapping_sub(reg(r2))))
            }
            Instruction::mov(r1, r0) => changes.push(StateChange::Reg(r0, reg(r1))),
            Instruction::inv(r1, r0) => changes.push(StateChange::Reg(r0, !reg(r1))),
            Instruction::neg(r1, r0) => changes.push(StateChange::Reg(r0, u16::MAX - reg(r1))),
            Instruction::addi(imm, r1, r0) => {
                changes.push(StateChange::Reg(r0, reg(r1).wrapping_sub(reg(imm))))
            }
            Instruction::shlu(imm, r1, r0) => changes.push(StateChange::Reg(r0, reg(r1) << imm)),
            Instruction::shru(imm, r1, r0) => changes.push(StateChange::Reg(r0, reg(r1) >> imm)),
            Instruction::not0(r1, r0) => {
                changes.push(StateChange::Reg(r0, select(reg(r1) != 0, 1, 0)))
            }
            Instruction::load_hi(hi, lo, r0) => changes.push(StateChange::Reg(
                r0,
                (((hi as u16) << 12) | ((lo as u16) << 8)) | (reg(r0) & 0b11111111),
            )),
            Instruction::load_lo(hi, lo, r0) => {
                changes.push(StateChange::Reg(r0, ((hi as u16) << 4) | (lo as u16)))
            }
        }

        if pc != pc_next {
            changes.push(StateChange::Pc(pc_next));
        }

        changes
    }
    pub fn test(&self, ref_changes: Vec<StateChange>) -> SimTestResult {
        //TODO call eval
        //TODO new result
        todo!()
    }

    pub fn commit(&mut self, changes: Vec<StateChange>) {
        for change in changes {
            match change {
                StateChange::Reg(addr, v) => self.state.reg[addr as usize] = v,
                StateChange::Mem(addr, v) => self.state.mem[addr as usize] = v,
                StateChange::Pc(pc) => self.state.pc = pc,
                StateChange::StackPtr(ptr) => self.state.stack_ptr = ptr,
                StateChange::Flags(flags) => self.state.flags = flags,
            }
        }
    }
}

pub struct SimTestResult {
    pub pass: bool,
    pub sim_changes: Vec<StateChange>,
    pub ref_changes: Vec<StateChange>,
}
impl SimTestResult {
    pub fn new(mut sim_changes: Vec<StateChange>, mut ref_changes: Vec<StateChange>) -> Self {
        sim_changes.sort(); //TODO StateChanges = sorted Vec<StateChange> + Eq
        ref_changes.sort();
        Self {
            pass: sim_changes == ref_changes,
            sim_changes,
            ref_changes,
        }
    }
    //TODO to string? debug?
    //TODO is_passed()
}
