use super::{REGISTER_COUNT, REG_INDEX_WIDTH, WORD_WIDTH};
use digital_design_code::{
    input_w, CircuitComponent, CircuitComponentEmu, CircuitWires, Wires, WiresU16, WiresU8,
};

#[derive(Clone)]
pub struct RegisterReadInput {
    pub regs: [Wires<WORD_WIDTH>; REGISTER_COUNT],
    pub source_a: Wires<REG_INDEX_WIDTH>,
    pub source_b: Wires<REG_INDEX_WIDTH>,
}

#[derive(Clone)]
pub struct RegisterReadOutput {
    pub source_a: Wires<WORD_WIDTH>,
    pub source_b: Wires<WORD_WIDTH>,
}

pub struct CpuRegisterRead;

impl CircuitComponent for CpuRegisterRead {
    type Input = RegisterReadInput;
    type Output = RegisterReadOutput;

    fn build(_input: &Self::Input) -> Self::Output {
        todo!("cpu_v2 register read implementation")
    }
}

pub struct CpuRegisterReadEmu;

impl CircuitComponentEmu<CpuRegisterRead> for CpuRegisterReadEmu {
    fn create(input: &RegisterReadInput) -> (Self, RegisterReadOutput) {
        let output = RegisterReadOutput {
            source_a: input_w(),
            source_b: input_w(),
        };
        let register_latency = input
            .regs
            .iter()
            .map(|reg| reg.get_max_latency_external())
            .max()
            .unwrap_or(0);
        let latency = register_latency
            .max(input.source_a.get_max_latency_external())
            .max(input.source_b.get_max_latency_external())
            + 1;
        output.source_a.set_latency_external(latency);
        output.source_b.set_latency_external(latency);
        (Self, output)
    }

    fn execute(
        &mut self,
        circuit: &mut CircuitWires,
        input: &RegisterReadInput,
        output: &RegisterReadOutput,
    ) {
        let source_a = input.source_a.get_u8(circuit) as usize;
        let source_b = input.source_b.get_u8(circuit) as usize;
        output
            .source_a
            .set_u16(circuit, input.regs[source_a].get_u16(circuit));
        output
            .source_b
            .set_u16(circuit, input.regs[source_b].get_u16(circuit));
    }
}
