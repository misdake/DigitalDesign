use super::{ExecOp, EXEC_OP_WIDTH, FLAGS_WIDTH, WORD_WIDTH};
use crate::semantics::{calc_flags, calc_flags_signed};
use digital_design_code::{
    input_w, CircuitComponent, CircuitComponentEmu, CircuitWires, Wires, WiresU16, WiresU8,
};

#[derive(Clone)]
pub struct ExecuteInput {
    pub pc: Wires<WORD_WIDTH>,
    pub source_a: Wires<WORD_WIDTH>,
    pub source_b: Wires<WORD_WIDTH>,
    pub immediate: Wires<WORD_WIDTH>,
    pub operation: Wires<EXEC_OP_WIDTH>,
}

#[derive(Clone)]
pub struct ExecuteOutput {
    pub result: Wires<WORD_WIDTH>,
    pub flags: Wires<FLAGS_WIDTH>,
    pub memory_address: Wires<WORD_WIDTH>,
    pub memory_write: Wires<WORD_WIDTH>,
    pub pc_target: Wires<WORD_WIDTH>,
    pub device_write: Wires<WORD_WIDTH>,
    pub halt_signal: Wires<WORD_WIDTH>,
}

pub struct CpuExecute;

impl CircuitComponent for CpuExecute {
    type Input = ExecuteInput;
    type Output = ExecuteOutput;

    fn build(_input: &Self::Input) -> Self::Output {
        todo!("cpu_v2 execute implementation")
    }
}

pub struct CpuExecuteEmu;

impl CircuitComponentEmu<CpuExecute> for CpuExecuteEmu {
    fn create(input: &ExecuteInput) -> (Self, ExecuteOutput) {
        let output = ExecuteOutput {
            result: input_w(),
            flags: input_w(),
            memory_address: input_w(),
            memory_write: input_w(),
            pc_target: input_w(),
            device_write: input_w(),
            halt_signal: input_w(),
        };
        let latency = input
            .pc
            .get_max_latency_external()
            .max(input.source_a.get_max_latency_external())
            .max(input.source_b.get_max_latency_external())
            .max(input.immediate.get_max_latency_external())
            .max(input.operation.get_max_latency_external())
            + 1;
        output.result.set_latency_external(latency);
        output.flags.set_latency_external(latency);
        output.memory_address.set_latency_external(latency);
        output.memory_write.set_latency_external(latency);
        output.pc_target.set_latency_external(latency);
        output.device_write.set_latency_external(latency);
        output.halt_signal.set_latency_external(latency);
        (Self, output)
    }

    fn execute(
        &mut self,
        circuit: &mut CircuitWires,
        input: &ExecuteInput,
        output: &ExecuteOutput,
    ) {
        let pc = input.pc.get_u16(circuit);
        let source_a = input.source_a.get_u16(circuit);
        let source_b = input.source_b.get_u16(circuit);
        let immediate = input.immediate.get_u16(circuit);
        let operation = ExecOp::from_raw(input.operation.get_u8(circuit));

        let mut result = 0;
        let mut flags = 0;
        let mut memory_address = 0;
        let mut pc_target = 0;

        match operation {
            ExecOp::Idle => {}
            ExecOp::PassA => {
                result = source_a;
                pc_target = source_a;
            }
            ExecOp::Inv => result = !source_a,
            ExecOp::Neg => result = (source_a as i16).wrapping_neg() as u16,
            ExecOp::NotZero => result = u16::from(source_a != 0),
            ExecOp::CountOnes => result = source_a.count_ones() as u16,
            ExecOp::Log2 => {
                result = if source_a == 0 {
                    0
                } else {
                    source_a.ilog2() as u16
                }
            }
            ExecOp::Lsl => result = source_a << immediate,
            ExecOp::Lsr => result = source_a >> immediate,
            ExecOp::Asr => result = ((source_a as i16) >> immediate) as u16,
            ExecOp::And => result = source_a & source_b,
            ExecOp::Or => result = source_a | source_b,
            ExecOp::Xor => result = source_a ^ source_b,
            ExecOp::Add => result = source_a.wrapping_add(source_b),
            ExecOp::Sub => result = source_a.wrapping_sub(source_b),
            ExecOp::AddImmediate => {
                result = source_a.wrapping_add(immediate);
                memory_address = result;
            }
            ExecOp::LoadHi => result = immediate | (source_a & 0x00ff),
            ExecOp::LoadLo => result = immediate,
            ExecOp::PcAdd => {
                result = pc.wrapping_add(immediate);
                pc_target = result;
            }
            ExecOp::CompareUnsigned => flags = calc_flags(source_a, source_b),
            ExecOp::CompareUnsignedImmediate => flags = calc_flags(source_a, immediate),
            ExecOp::CompareSigned => flags = calc_flags_signed(source_a, source_b),
            ExecOp::CompareSignedImmediate => flags = calc_flags_signed(source_a, immediate),
            ExecOp::CallRelative => {
                result = pc.wrapping_add(1);
                pc_target = pc.wrapping_add(immediate);
            }
            ExecOp::CallAbsolute => {
                result = pc.wrapping_add(1);
                memory_address = immediate;
            }
            ExecOp::CallRegister => {
                result = pc.wrapping_add(1);
                pc_target = source_a;
            }
            ExecOp::Max => unreachable!(),
        }

        output.result.set_u16(circuit, result);
        output.flags.set_u8(circuit, flags);
        output.memory_address.set_u16(circuit, memory_address);
        output.memory_write.set_u16(circuit, source_b);
        output.pc_target.set_u16(circuit, pc_target);
        output.device_write.set_u16(circuit, source_a);
        output.halt_signal.set_u16(circuit, source_a);
    }
}
