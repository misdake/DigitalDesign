use super::{REGISTER_COUNT, REG_INDEX_WIDTH, WORD_WIDTH};
use digital_design_circuit::{
    input_w, mux16_w, CircuitComponent, CircuitComponentEmu, CircuitWires, Wires, WiresU16, WiresU8,
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

    fn build(input: &Self::Input) -> Self::Output {
        RegisterReadOutput {
            source_a: mux16_w(&input.regs, input.source_a),
            source_b: mux16_w(&input.regs, input.source_b),
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use digital_design_circuit::{build_circuit, input_w};

    #[test]
    fn reads_both_register_ports() {
        let (mut circuit, (regs, source_a, source_b, output)) = build_circuit(|| {
            let regs = [(); REGISTER_COUNT].map(|_| input_w());
            let source_a = input_w();
            let source_b = input_w();
            let output = CpuRegisterRead::build(&RegisterReadInput {
                regs,
                source_a,
                source_b,
            });
            (regs, source_a, source_b, output)
        });

        for (index, reg) in regs.iter().enumerate() {
            reg.set_u16(&mut circuit, 0x1000 + index as u16);
        }
        source_a.set_u8(&mut circuit, 3);
        source_b.set_u8(&mut circuit, 14);
        circuit.execute_gates();

        assert_eq!(output.source_a.get_u16(&circuit), 0x1003);
        assert_eq!(output.source_b.get_u16(&circuit), 0x100e);
    }
}
