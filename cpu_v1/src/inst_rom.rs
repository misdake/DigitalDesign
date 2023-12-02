use crate::{CpuComponent, CpuComponentEmu};
use digital_design_code::{input_w, mux256_w, Wires};

#[derive(Clone)]
pub struct CpuInstInput {
    pub inst: [Wires<8>; 256],
    pub pc: Wires<8>,
}
#[derive(Clone)]
pub struct CpuInstOutput {
    pub inst: Wires<8>,
}

pub struct CpuInstRom;
impl CpuComponent for CpuInstRom {
    type Input = CpuInstInput;
    type Output = CpuInstOutput;

    fn build(input: &CpuInstInput) -> CpuInstOutput {
        let output = mux256_w(input.inst.as_slice(), input.pc);
        CpuInstOutput { inst: output }
    }
}

pub struct CpuInstRomEmu;
impl CpuComponentEmu<CpuInstRom> for CpuInstRomEmu {
    fn init_output(i: &CpuInstInput) -> CpuInstOutput {
        let output = CpuInstOutput { inst: input_w() };
        output.inst.set_latency(i.pc.get_max_latency() + 10); //TODO accurate latency
        output
    }
    fn execute(input: &CpuInstInput, output: &CpuInstOutput) {
        let pc = input.pc.get_u8();
        let inst = input.inst[pc as usize].get_u8();
        output.inst.set_u8(inst);
    }
}
