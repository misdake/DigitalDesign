use crate::cpu_v1::{CpuComponent, CpuComponentEmu};
use crate::{input_w, mux256_w, Wires};

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
    fn init_output() -> CpuInstOutput {
        CpuInstOutput { inst: input_w() }
    }
    fn execute(input: &CpuInstInput, output: &CpuInstOutput) {
        let pc = input.pc.get_u8();
        let inst = input.inst[pc as usize].get_u8();
        output.inst.set_u8(inst);
    }
}
