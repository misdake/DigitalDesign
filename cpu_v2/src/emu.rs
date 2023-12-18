use crate::isa::Instruction;

pub struct EmuEnv {
    inst: Box<[Instruction; 65536]>,
    state: EmuState,
}

struct EmuState {
    reg: [u16; 16],
    mem: Box<[u16; 65536]>,

    pc: u16,
    stack_ptr: u16,
    flags: u8,
}

impl Default for EmuState {
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
    Pc(u16),
    StackPtr(u16),
    Reg { addr: u8, value: u16 },
    Mem { addr: u16, value: u16 },
    Flags(u8),
}

impl EmuEnv {
    pub fn new(inst: Box<[Instruction; 65536]>) -> EmuEnv {
        Self {
            inst,
            state: EmuState::default(),
        }
    }

    pub fn get_state(&self) -> &EmuState {
        &self.state
    }

    pub fn eval(&self) -> Vec<StateChange> {
        let mut changes = vec![];

        let pc = self.state.pc;
        let inst = self.inst[pc as usize];
        let mut pc_next = pc + 1;
        let reg = &self.state.reg;

        changes
    }
    pub fn test(&self, ref_changes: Vec<StateChange>) -> EmuTestResult {
        //TODO call eval
        //TODO new result
        todo!()
    }

    pub fn commit(&mut self, changes: &StateChange) {
        //TODO just a big match
    }
}

pub struct EmuTestResult {
    pub pass: bool,
    pub emu_changes: Vec<StateChange>,
    pub ref_changes: Vec<StateChange>,
}
impl EmuTestResult {
    pub fn new(mut emu_changes: Vec<StateChange>, mut ref_changes: Vec<StateChange>) -> Self {
        emu_changes.sort();
        ref_changes.sort();
        Self {
            pass: emu_changes == ref_changes,
            emu_changes,
            ref_changes,
        }
    }
    //TODO to string? debug?
    //TODO is_passed()
}
