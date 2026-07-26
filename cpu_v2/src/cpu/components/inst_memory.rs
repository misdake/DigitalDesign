use super::{MEMORY_WORDS, WORD_WIDTH};
use digital_design_code::{
    input_w, CircuitComponent, CircuitComponentEmu, CircuitWires, Wires, WiresU16,
};
use std::rc::Rc;

pub type InstructionImage = Rc<[u16]>;

#[derive(Clone)]
pub struct InstMemoryInput {
    pub address: Wires<WORD_WIDTH>,
    pub image: InstructionImage,
}

#[derive(Clone)]
pub struct InstMemoryOutput {
    pub instruction: Wires<WORD_WIDTH>,
}

pub struct CpuInstMemory;

impl CircuitComponent for CpuInstMemory {
    type Input = InstMemoryInput;
    type Output = InstMemoryOutput;

    fn build(_input: &Self::Input) -> Self::Output {
        unreachable!("cpu_v2 instruction memory must use CpuInstMemoryEmu")
    }
}

pub struct CpuInstMemoryEmu;

impl CircuitComponentEmu<CpuInstMemory> for CpuInstMemoryEmu {
    fn create(input: &InstMemoryInput) -> (Self, InstMemoryOutput) {
        assert!(
            input.image.len() <= MEMORY_WORDS,
            "instruction image exceeds 64K words"
        );
        let instruction = input_w();
        instruction.set_latency_external(input.address.get_max_latency_external() + 1);
        (Self, InstMemoryOutput { instruction })
    }

    fn execute(
        &mut self,
        circuit: &mut CircuitWires,
        input: &InstMemoryInput,
        output: &InstMemoryOutput,
    ) {
        let address = input.address.get_u16(circuit) as usize;
        let instruction = input.image.get(address).copied().unwrap_or(0);
        output.instruction.set_u16(circuit, instruction);
    }
}
