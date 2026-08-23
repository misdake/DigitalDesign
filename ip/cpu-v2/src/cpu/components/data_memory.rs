use super::{MEMORY_WORDS, WORD_WIDTH};
use digital_design_circuit::{
    input_w, CircuitComponent, CircuitComponentEmu, CircuitWires, Wire, Wires, WiresU16,
};

#[derive(Clone)]
pub struct DataMemoryInput {
    pub address: Wires<WORD_WIDTH>,
    pub read_enable: Wire,
    pub write_enable: Wire,
    pub write_data: Wires<WORD_WIDTH>,
}

#[derive(Clone)]
pub struct DataMemoryOutput {
    pub read_data: Wires<WORD_WIDTH>,
}

pub struct CpuDataMemory;

impl CircuitComponent for CpuDataMemory {
    type Input = DataMemoryInput;
    type Output = DataMemoryOutput;

    fn build(_input: &Self::Input) -> Self::Output {
        unreachable!("cpu_v2 data memory must use CpuDataMemoryEmu")
    }
}

pub struct CpuDataMemoryEmu {
    memory: Box<[u16]>,
    pending_write: Option<(usize, u16)>,
}

impl CircuitComponentEmu<CpuDataMemory> for CpuDataMemoryEmu {
    fn create(input: &DataMemoryInput) -> (Self, DataMemoryOutput) {
        let read_data = input_w();
        let latency = input
            .address
            .get_max_latency_external()
            .max(input.read_enable.get_latency_external())
            .max(input.write_enable.get_latency_external())
            .max(input.write_data.get_max_latency_external())
            + 1;
        read_data.set_latency_external(latency);
        (
            Self {
                memory: vec![0; MEMORY_WORDS].into_boxed_slice(),
                pending_write: None,
            },
            DataMemoryOutput { read_data },
        )
    }

    fn execute(
        &mut self,
        circuit: &mut CircuitWires,
        input: &DataMemoryInput,
        output: &DataMemoryOutput,
    ) {
        let address = input.address.get_u16(circuit) as usize;
        let read_data = if input.read_enable.is_one(circuit) {
            self.memory[address]
        } else {
            0
        };
        output.read_data.set_u16(circuit, read_data);

        self.pending_write = input
            .write_enable
            .is_one(circuit)
            .then(|| (address, input.write_data.get_u16(circuit)));
    }

    fn clock(
        &mut self,
        _circuit: &mut CircuitWires,
        _input: &DataMemoryInput,
        _output: &DataMemoryOutput,
    ) {
        if let Some((address, value)) = self.pending_write.take() {
            self.memory[address] = value;
        }
    }
}
