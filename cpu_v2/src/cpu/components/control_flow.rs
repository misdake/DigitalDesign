use super::{set_wire, PcSrc, FLAGS_WIDTH, PC_SRC_WIDTH, WORD_WIDTH};
use digital_design_code::{
    input as input_wire, input_w, CircuitComponent, CircuitComponentEmu, CircuitWires, Wire, Wires,
    WiresU16, WiresU8,
};

#[derive(Clone)]
pub struct ControlFlowInput {
    pub reset: Wire,
    pub pc: Wires<WORD_WIDTH>,
    pub flags: Wires<FLAGS_WIDTH>,
    pub halted: Wire,

    pub flags_write_enable: Wire,
    pub flags_write: Wires<FLAGS_WIDTH>,

    pub pc_source: Wires<PC_SRC_WIDTH>,
    pub condition_mask: Wires<FLAGS_WIDTH>,
    pub pc_target: Wires<WORD_WIDTH>,
    pub memory_target: Wires<WORD_WIDTH>,

    pub halt_enable: Wire,
    pub halt_signal: Wires<WORD_WIDTH>,
}

#[derive(Clone)]
pub struct ControlFlowOutput {
    pub pc: Wires<WORD_WIDTH>,
    pub flags: Wires<FLAGS_WIDTH>,
    pub halted: Wire,
    pub halt_signal: Wires<WORD_WIDTH>,
}

pub struct CpuControlFlow;

impl CircuitComponent for CpuControlFlow {
    type Input = ControlFlowInput;
    type Output = ControlFlowOutput;

    fn build(_input: &Self::Input) -> Self::Output {
        todo!("cpu_v2 control flow implementation")
    }
}

pub struct CpuControlFlowEmu;

impl CircuitComponentEmu<CpuControlFlow> for CpuControlFlowEmu {
    fn create(input: &ControlFlowInput) -> (Self, ControlFlowOutput) {
        let output = ControlFlowOutput {
            pc: input_w(),
            flags: input_w(),
            halted: input_wire(),
            halt_signal: input_w(),
        };
        let latency = input
            .pc
            .get_max_latency_external()
            .max(input.flags.get_max_latency_external())
            .max(input.halted.get_latency_external())
            .max(input.reset.get_latency_external())
            .max(input.flags_write_enable.get_latency_external())
            .max(input.flags_write.get_max_latency_external())
            .max(input.pc_source.get_max_latency_external())
            .max(input.condition_mask.get_max_latency_external())
            .max(input.pc_target.get_max_latency_external())
            .max(input.memory_target.get_max_latency_external())
            .max(input.halt_enable.get_latency_external())
            .max(input.halt_signal.get_max_latency_external())
            + 1;
        output.pc.set_latency_external(latency);
        output.flags.set_latency_external(latency);
        output.halted.set_latency_external(latency);
        output.halt_signal.set_latency_external(latency);
        (Self, output)
    }

    fn execute(
        &mut self,
        circuit: &mut CircuitWires,
        input: &ControlFlowInput,
        output: &ControlFlowOutput,
    ) {
        let current_pc = input.pc.get_u16(circuit);
        let current_flags = input.flags.get_u8(circuit);

        let (pc, flags, halted, halt_signal) = if input.reset.is_one(circuit) {
            (0, 0, false, 0)
        } else if input.halted.is_one(circuit) || input.halt_enable.is_one(circuit) {
            (
                current_pc,
                current_flags,
                true,
                input.halt_signal.get_u16(circuit),
            )
        } else {
            let flags = if input.flags_write_enable.is_one(circuit) {
                input.flags_write.get_u8(circuit)
            } else {
                current_flags
            };
            let condition = input.condition_mask.get_u8(circuit);
            let branch_taken = current_flags & condition != 0;
            let pc = match PcSrc::from_raw(input.pc_source.get_u8(circuit)) {
                PcSrc::Next => current_pc.wrapping_add(1),
                PcSrc::Execute if branch_taken => input.pc_target.get_u16(circuit),
                PcSrc::Memory if branch_taken => input.memory_target.get_u16(circuit),
                PcSrc::Execute | PcSrc::Memory => current_pc.wrapping_add(1),
            };
            (pc, flags, false, 0)
        };

        output.pc.set_u16(circuit, pc);
        output.flags.set_u8(circuit, flags);
        set_wire(circuit, output.halted, halted);
        output.halt_signal.set_u16(circuit, halt_signal);
    }
}
