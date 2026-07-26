use super::{WbSrc, REGISTER_COUNT, REG_INDEX_WIDTH, WB_SRC_WIDTH, WORD_WIDTH};
use digital_design_code::{
    input_w, CircuitComponent, CircuitComponentEmu, CircuitWires, Wire, Wires, WiresU16, WiresU8,
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

    fn build(_input: &Self::Input) -> Self::Output {
        todo!("cpu_v2 writeback implementation")
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
