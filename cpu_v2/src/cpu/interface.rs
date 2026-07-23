use digital_design_code::{Reg, Regs, Wire, Wires};

pub const WORD_WIDTH: usize = 16;
pub const REGISTER_COUNT: usize = 16;
pub const FLAGS_WIDTH: usize = 3;

#[derive(Clone)]
pub struct CpuV2State {
    pub pc: Regs<WORD_WIDTH>,
    pub regs: [Regs<WORD_WIDTH>; REGISTER_COUNT],
    pub flags: Regs<FLAGS_WIDTH>,
    pub halted: Reg,
}

#[derive(Clone)]
pub struct CpuV2Input {
    pub reset: Wire,
    pub instruction: Wires<WORD_WIDTH>,
    pub data_read: Wires<WORD_WIDTH>,
    pub device_read: Wires<WORD_WIDTH>,
}

#[derive(Clone)]
pub struct CpuV2BuildInput {
    pub state: CpuV2State,
    pub ports: CpuV2Input,
}

#[derive(Clone)]
pub(super) struct CpuV2NextState {
    pub pc: Wires<WORD_WIDTH>,
    pub regs: [Wires<WORD_WIDTH>; REGISTER_COUNT],
    pub flags: Wires<FLAGS_WIDTH>,
    pub halted: Wire,
}

#[derive(Clone)]
pub struct CpuV2Output {
    pub instruction_addr: Wires<WORD_WIDTH>,

    pub data_addr: Wires<WORD_WIDTH>,
    pub data_read_enable: Wire,
    pub data_write_enable: Wire,
    pub data_write: Wires<WORD_WIDTH>,

    pub device_index: Wires<4>,
    pub device_channel: Wires<4>,
    pub device_read_enable: Wire,
    pub device_write_enable: Wire,
    pub device_write: Wires<WORD_WIDTH>,

    pub halted: Wire,
    pub halt_signal: Wires<WORD_WIDTH>,

    #[allow(dead_code)] // Connected now and consumed by the upcoming emu implementation.
    pub(super) next_state: CpuV2NextState,
}
