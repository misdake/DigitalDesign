use super::{WbSrc, REGISTER_COUNT, REG_INDEX_WIDTH, WB_SRC_WIDTH, WORD_WIDTH};
use digital_design_circuit::{
    decode4, input_w, input_w_const, mux2_w, CircuitComponent, CircuitComponentEmu, CircuitWires,
    Wire, Wires, WiresU16, WiresU8,
};

#[derive(Clone)]
pub struct WritebackInput {
    pub reset: Wire,
    pub regs: [Wires<WORD_WIDTH>; REGISTER_COUNT],
    pub destination: Wires<REG_INDEX_WIDTH>,
    pub write_enable: Wire,
    pub source: Wires<WB_SRC_WIDTH>,
    pub execute_data: Wires<WORD_WIDTH>,
    pub memory_data: Wires<WORD_WIDTH>,
    pub device_data: Wires<WORD_WIDTH>,
}

#[derive(Clone)]
pub struct WritebackOutput {
    pub regs: [Wires<WORD_WIDTH>; REGISTER_COUNT],
}

pub struct CpuWriteback;

impl CircuitComponent for CpuWriteback {
    type Input = WritebackInput;
    type Output = WritebackOutput;

    fn build(input: &Self::Input) -> Self::Output {
        let source_execute = input.source.eq_const(WbSrc::Execute as u8);
        let source_memory = input.source.eq_const(WbSrc::Memory as u8);
        let source_device = input.source.eq_const(WbSrc::Device as u8);
        let write_data = (source_execute.expand() & input.execute_data)
            | (source_memory.expand() & input.memory_data)
            | (source_device.expand() & input.device_data);
        let destination = decode4(input.destination);
        let zero = input_w_const(0);

        WritebackOutput {
            regs: std::array::from_fn(|index| {
                let write_enable = input.write_enable & destination[index];
                let next = mux2_w(input.regs[index], write_data, write_enable);
                mux2_w(next, zero, input.reset)
            }),
        }
    }
}

pub struct CpuWritebackEmu;

impl CircuitComponentEmu<CpuWriteback> for CpuWritebackEmu {
    fn create(input: &WritebackInput) -> (Self, WritebackOutput) {
        let output = WritebackOutput {
            regs: [(); REGISTER_COUNT].map(|_| input_w()),
        };
        let register_latency = input
            .regs
            .iter()
            .map(|reg| reg.get_max_latency_external())
            .max()
            .unwrap_or(0);
        let latency = register_latency
            .max(input.reset.get_latency_external())
            .max(input.destination.get_max_latency_external())
            .max(input.write_enable.get_latency_external())
            .max(input.source.get_max_latency_external())
            .max(input.execute_data.get_max_latency_external())
            .max(input.memory_data.get_max_latency_external())
            .max(input.device_data.get_max_latency_external())
            + 1;
        for reg in output.regs {
            reg.set_latency_external(latency);
        }
        (Self, output)
    }

    fn execute(
        &mut self,
        circuit: &mut CircuitWires,
        input: &WritebackInput,
        output: &WritebackOutput,
    ) {
        let mut regs = input.regs.map(|reg| reg.get_u16(circuit));

        if input.reset.is_one(circuit) {
            regs.fill(0);
        } else if input.write_enable.is_one(circuit) {
            let data = match WbSrc::from_raw(input.source.get_u8(circuit)) {
                WbSrc::Execute => input.execute_data.get_u16(circuit),
                WbSrc::Memory => input.memory_data.get_u16(circuit),
                WbSrc::Device => input.device_data.get_u16(circuit),
            };
            regs[input.destination.get_u8(circuit) as usize] = data;
        }

        for (output, value) in output.regs.iter().zip(regs) {
            output.set_u16(circuit, value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use digital_design_circuit::{build_circuit, input, input_w};

    #[test]
    fn writes_selected_register_and_resets_all() {
        let (
            mut circuit,
            (reset, regs, destination, write_enable, source, execute_data, memory_data, output),
        ) = build_circuit(|| {
            let reset = input();
            let regs = [(); REGISTER_COUNT].map(|_| input_w());
            let destination = input_w();
            let write_enable = input();
            let source = input_w();
            let execute_data = input_w();
            let memory_data = input_w();
            let output = CpuWriteback::build(&WritebackInput {
                reset,
                regs,
                destination,
                write_enable,
                source,
                execute_data,
                memory_data,
                device_data: input_w(),
            });
            (
                reset,
                regs,
                destination,
                write_enable,
                source,
                execute_data,
                memory_data,
                output,
            )
        });

        for (index, reg) in regs.iter().enumerate() {
            reg.set_u16(&mut circuit, index as u16);
        }
        destination.set_u8(&mut circuit, 5);
        write_enable.set(&mut circuit, 1);
        source.set_u8(&mut circuit, WbSrc::Memory as u8);
        execute_data.set_u16(&mut circuit, 0x1111);
        memory_data.set_u16(&mut circuit, 0x2222);
        circuit.execute_gates();
        assert_eq!(output.regs[5].get_u16(&circuit), 0x2222);
        assert_eq!(output.regs[4].get_u16(&circuit), 4);

        reset.set(&mut circuit, 1);
        circuit.execute_gates();
        assert!(output.regs.iter().all(|reg| reg.get_u16(&circuit) == 0));
    }
}
