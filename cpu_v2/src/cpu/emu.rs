use super::*;
use digital_design_code::{CircuitComponentEmu, CircuitWires};

pub struct CpuInstMemoryEmu;

impl CircuitComponentEmu<CpuInstMemory> for CpuInstMemoryEmu {
    fn create(_input: &InstMemoryInput) -> (Self, InstMemoryOutput) {
        todo!("cpu_v2 instruction memory emulation")
    }

    fn execute(
        &mut self,
        _circuit: &mut CircuitWires,
        _input: &InstMemoryInput,
        _output: &InstMemoryOutput,
    ) {
        todo!("cpu_v2 instruction memory emulation")
    }
}

pub struct CpuDecoderEmu;

impl CircuitComponentEmu<CpuDecoder> for CpuDecoderEmu {
    fn create(_input: &DecoderInput) -> (Self, DecoderOutput) {
        todo!("cpu_v2 decoder emulation")
    }

    fn execute(
        &mut self,
        _circuit: &mut CircuitWires,
        _input: &DecoderInput,
        _output: &DecoderOutput,
    ) {
        todo!("cpu_v2 decoder emulation")
    }
}

pub struct CpuRegisterReadEmu;

impl CircuitComponentEmu<CpuRegisterRead> for CpuRegisterReadEmu {
    fn create(_input: &RegisterReadInput) -> (Self, RegisterReadOutput) {
        todo!("cpu_v2 register read emulation")
    }

    fn execute(
        &mut self,
        _circuit: &mut CircuitWires,
        _input: &RegisterReadInput,
        _output: &RegisterReadOutput,
    ) {
        todo!("cpu_v2 register read emulation")
    }
}

pub struct CpuExecuteEmu;

impl CircuitComponentEmu<CpuExecute> for CpuExecuteEmu {
    fn create(_input: &ExecuteInput) -> (Self, ExecuteOutput) {
        todo!("cpu_v2 execute emulation")
    }

    fn execute(
        &mut self,
        _circuit: &mut CircuitWires,
        _input: &ExecuteInput,
        _output: &ExecuteOutput,
    ) {
        todo!("cpu_v2 execute emulation")
    }
}

pub struct CpuDataMemoryEmu;

impl CircuitComponentEmu<CpuDataMemory> for CpuDataMemoryEmu {
    fn create(_input: &DataMemoryInput) -> (Self, DataMemoryOutput) {
        todo!("cpu_v2 data memory emulation")
    }

    fn execute(
        &mut self,
        _circuit: &mut CircuitWires,
        _input: &DataMemoryInput,
        _output: &DataMemoryOutput,
    ) {
        todo!("cpu_v2 data memory emulation")
    }
}

pub struct CpuWritebackEmu;

impl CircuitComponentEmu<CpuWriteback> for CpuWritebackEmu {
    fn create(_input: &WritebackInput) -> (Self, WritebackOutput) {
        todo!("cpu_v2 writeback emulation")
    }

    fn execute(
        &mut self,
        _circuit: &mut CircuitWires,
        _input: &WritebackInput,
        _output: &WritebackOutput,
    ) {
        todo!("cpu_v2 writeback emulation")
    }
}

pub struct CpuControlFlowEmu;

impl CircuitComponentEmu<CpuControlFlow> for CpuControlFlowEmu {
    fn create(_input: &ControlFlowInput) -> (Self, ControlFlowOutput) {
        todo!("cpu_v2 control flow emulation")
    }

    fn execute(
        &mut self,
        _circuit: &mut CircuitWires,
        _input: &ControlFlowInput,
        _output: &ControlFlowOutput,
    ) {
        todo!("cpu_v2 control flow emulation")
    }
}
