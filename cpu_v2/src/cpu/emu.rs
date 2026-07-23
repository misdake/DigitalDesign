use super::{
    CpuComponent, CpuComponentEmu, CpuV2BuildInput, CpuV2NextState, CpuV2Output, CpuV2State,
    FLAGS_WIDTH, REGISTER_COUNT, WORD_WIDTH,
};
use digital_design_code::{input, input_w, reg, reg_w, CircuitWires};

pub struct CpuV2;

impl CpuV2 {
    pub fn create_state() -> CpuV2State {
        CpuV2State {
            pc: reg_w(),
            regs: [(); REGISTER_COUNT].map(|_| reg_w()),
            flags: reg_w(),
            halted: reg(),
        }
    }
}

impl CpuComponent for CpuV2 {
    type Input = CpuV2BuildInput;
    type Output = CpuV2Output;

    fn build(_input: &Self::Input) -> Self::Output {
        todo!("cpu_v2 hardware implementation")
    }
}

pub struct CpuV2Emu;

impl CpuComponentEmu<CpuV2> for CpuV2Emu {
    fn init_output(build_input: &CpuV2BuildInput) -> CpuV2Output {
        let next_state = CpuV2NextState {
            pc: input_w::<WORD_WIDTH>(),
            regs: [(); REGISTER_COUNT].map(|_| input_w::<WORD_WIDTH>()),
            flags: input_w::<FLAGS_WIDTH>(),
            halted: input(),
        };

        build_input.state.pc.set_in(next_state.pc);
        for (reg, next) in build_input.state.regs.iter().zip(next_state.regs) {
            reg.set_in(next);
        }
        build_input.state.flags.set_in(next_state.flags);
        build_input.state.halted.set_in(next_state.halted);

        CpuV2Output {
            instruction_addr: input_w(),
            data_addr: input_w(),
            data_read_enable: input(),
            data_write_enable: input(),
            data_write: input_w(),
            device_index: input_w(),
            device_channel: input_w(),
            device_read_enable: input(),
            device_write_enable: input(),
            device_write: input_w(),
            halted: input(),
            halt_signal: input_w(),
            next_state,
        }
    }

    fn execute(_circuit: &mut CircuitWires, _input: &CpuV2BuildInput, _output: &CpuV2Output) {
        todo!("cpu_v2 emulation implementation")
    }
}
