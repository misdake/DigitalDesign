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
    flag_s: u8,
    flag_z: u8,
}

// pub fn export_emu_state(&CpuState) -> EmuState { ... }

// impl EmuState {
//     pub fn diff(a: &EmuState, b: &EmuState) -> String { ...??? }
// }

impl Default for EmuState {
    fn default() -> Self {
        Self {
            reg: [0; 16],
            mem: Box::new([0; 65536]),
            pc: 0,
            stack_ptr: 0,
            flag_s: 0,
            flag_z: 0,
        }
    }
}
