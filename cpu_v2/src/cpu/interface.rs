use super::InstructionImage;
use digital_design_code::{reg, reg_w, Reg, Regs, Wire, Wires};

pub const WORD_WIDTH: usize = 16;
pub const REGISTER_COUNT: usize = 16;
pub const FLAGS_WIDTH: usize = 3;

/// Clocked architectural state owned by the CPU data path.
///
/// Instruction and data memory are separate components selected by
/// [`CpuV2Design`](super::CpuV2Design), not fields in this register state.
#[derive(Clone)]
pub struct CpuV2State {
    pub pc: Regs<WORD_WIDTH>,
    pub regs: [Regs<WORD_WIDTH>; REGISTER_COUNT],
    pub flags: Regs<FLAGS_WIDTH>,
    pub halted: Reg,
}

impl CpuV2State {
    pub fn create() -> Self {
        Self {
            pc: reg_w(),
            regs: [(); REGISTER_COUNT].map(|_| reg_w()),
            flags: reg_w(),
            halted: reg(),
        }
    }
}

/// External CPU inputs. Device behavior is outside the CPU implementation.
#[derive(Clone)]
pub struct CpuV2Input {
    pub reset: Wire,
    pub device_read: Wires<WORD_WIDTH>,
}

#[derive(Clone)]
pub struct CpuV2BuildInput {
    pub state: CpuV2State,
    pub ports: CpuV2Input,
    pub instruction_image: InstructionImage,
}

/// External CPU outputs. Memory stays inside the selected CPU design.
#[derive(Clone)]
pub struct CpuV2Output {
    pub device_index: Wires<4>,
    pub device_channel: Wires<4>,
    pub device_read_enable: Wire,
    pub device_write_enable: Wire,
    pub device_write: Wires<WORD_WIDTH>,

    pub halted: Wire,
    pub halt_signal: Wires<WORD_WIDTH>,
}
