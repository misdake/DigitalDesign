use super::{FLAGS_WIDTH, REGISTER_COUNT, WORD_WIDTH};
use digital_design_code::{CircuitComponent, Wire, Wires};
use std::rc::Rc;

pub const REG_INDEX_WIDTH: usize = 4;
pub const EXEC_OP_WIDTH: usize = 6;
pub const WB_SRC_WIDTH: usize = 2;
pub const PC_SRC_WIDTH: usize = 3;

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

#[derive(Clone)]
pub struct DecoderInput {
    pub instruction: Wires<WORD_WIDTH>,
}

#[derive(Clone)]
pub struct DecoderOutput {
    pub source_a: Wires<REG_INDEX_WIDTH>,
    pub source_b: Wires<REG_INDEX_WIDTH>,
    pub destination: Wires<REG_INDEX_WIDTH>,
    pub immediate: Wires<WORD_WIDTH>,
    pub execute_operation: Wires<EXEC_OP_WIDTH>,

    pub register_write_enable: Wire,
    pub writeback_source: Wires<WB_SRC_WIDTH>,
    pub flags_write_enable: Wire,

    pub memory_read_enable: Wire,
    pub memory_write_enable: Wire,

    pub pc_source: Wires<PC_SRC_WIDTH>,
    pub condition_mask: Wires<FLAGS_WIDTH>,

    pub device_index: Wires<4>,
    pub device_channel: Wires<4>,
    pub device_read_enable: Wire,
    pub device_write_enable: Wire,

    pub halt_enable: Wire,
}

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

pub struct CpuInstMemory;

impl CircuitComponent for CpuInstMemory {
    type Input = InstMemoryInput;
    type Output = InstMemoryOutput;

    fn build(_input: &Self::Input) -> Self::Output {
        todo!("cpu_v2 instruction memory implementation")
    }
}

pub struct CpuDecoder;

impl CircuitComponent for CpuDecoder {
    type Input = DecoderInput;
    type Output = DecoderOutput;

    fn build(_input: &Self::Input) -> Self::Output {
        todo!("cpu_v2 decoder implementation")
    }
}

pub struct CpuRegisterRead;

impl CircuitComponent for CpuRegisterRead {
    type Input = RegisterReadInput;
    type Output = RegisterReadOutput;

    fn build(_input: &Self::Input) -> Self::Output {
        todo!("cpu_v2 register read implementation")
    }
}

pub struct CpuExecute;

impl CircuitComponent for CpuExecute {
    type Input = ExecuteInput;
    type Output = ExecuteOutput;

    fn build(_input: &Self::Input) -> Self::Output {
        todo!("cpu_v2 execute implementation")
    }
}

pub struct CpuDataMemory;

impl CircuitComponent for CpuDataMemory {
    type Input = DataMemoryInput;
    type Output = DataMemoryOutput;

    fn build(_input: &Self::Input) -> Self::Output {
        todo!("cpu_v2 data memory implementation")
    }
}

pub struct CpuWriteback;

impl CircuitComponent for CpuWriteback {
    type Input = WritebackInput;
    type Output = WritebackOutput;

    fn build(_input: &Self::Input) -> Self::Output {
        todo!("cpu_v2 writeback implementation")
    }
}

pub struct CpuControlFlow;

impl CircuitComponent for CpuControlFlow {
    type Input = ControlFlowInput;
    type Output = ControlFlowOutput;

    fn build(_input: &Self::Input) -> Self::Output {
        todo!("cpu_v2 control flow implementation")
    }
}
