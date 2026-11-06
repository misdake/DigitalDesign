//! Selected FPU execution with shared leaves and independent standalone peers.
use digital_design_circuit::{CircuitWires, Wire, Wires};
use digital_design_hardware::{
    resources::components::BsramBlocks, HardwareIdentity, Module, ModuleIo, TargetResourceRequest,
    VerilogDependency, VerilogIdentity,
};

pub(crate) mod encoding;
mod frontend;
pub mod lut;
mod multiplier;
mod register_file;
mod scalar_alu;
mod special_ports;
mod standalone;
pub use frontend::*;
pub use multiplier::*;
pub use register_file::*;
pub use scalar_alu::*;
pub use special_ports::*;
pub use standalone::*;

#[derive(Clone, ModuleIo)]
pub struct CpuV3FpuInput {
    pub abort: Wire,
    pub word_valid: Wire,
    pub word: Wires<16>,
    pub ext_access: Wire,
    pub ext_write_enable: Wire,
    pub ext_write_address: Wires<9>,
    pub ext_write_data: Wires<32>,
    pub ext_read_address: Wires<9>,
    pub integer_in_valid: Wire,
    pub integer_in_a: Wires<16>,
    pub integer_in_b: Wires<16>,
}

#[derive(Clone, ModuleIo)]
pub struct CpuV3FpuOutput {
    pub busy: Wire,
    pub flag_lt: Wire,
    pub flag_eq: Wire,
    pub flag_gt: Wire,
    pub instr_complete: Wire,
    pub ext_read_data: Wires<32>,
    pub integer_out_valid: Wire,
    pub integer_out_product: Wires<32>,
}

/// FPU v2: two-word frontend, mirrored RF, direct ALU writes and one shared
/// product stage. Ordinary lanes share one return slot; special functions
/// use a fixed consumer calendar. The core owns external FLD/FST accesses.
pub struct CpuV3Fpu;

impl HardwareIdentity for CpuV3Fpu {
    const TARGET_RESOURCE_LEAF: bool = false;

    fn verilog_identity() -> VerilogIdentity {
        VerilogIdentity::new("CpuV3Fpu").namespace(["components", "cpu", "cpu_v3"])
    }
}

mod execution;
mod execution_tb;
pub use execution::CpuV3FpuState;

impl Module for CpuV3Fpu {
    type Input = CpuV3FpuInput;
    type Output = CpuV3FpuOutput;
    type EmuState = CpuV3FpuState;

    const USES_MAIN_CLOCK: bool = true;

    fn create_emu(_input: &Self::Input, _output: &Self::Output) -> Self::EmuState {
        Self::EmuState::default()
    }

    fn execute_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        input: &Self::Input,
        output: &Self::Output,
    ) {
        let input = input.sample(circuit);
        output.drive(circuit, &state.comb(&input));
    }

    fn clock_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        input: &Self::Input,
        _output: &Self::Output,
    ) {
        let input = input.sample(circuit);
        state.tick(&input);
    }

    fn verilog_source() -> Option<String> {
        Some(include_str!("cpu_v3_fpu.v").to_string())
    }

    fn verilog_dependencies() -> Vec<VerilogDependency> {
        vec![
            VerilogDependency::new::<CpuV3FpuFrontend>("frontend"),
            VerilogDependency::new::<execution::CpuV3FpuExecRf>("rf"),
            VerilogDependency::new::<CpuV3FpuScalarAlu>("shared_alu"),
            VerilogDependency::new::<execution::CpuV3FpuExecSpecial>("special_path"),
            VerilogDependency::new::<execution::CpuV3FpuExecMul>("mul_pipe"),
        ]
    }

    fn verilog_testbench() -> Option<String> {
        Some(include_str!("cpu_v3_fpu_tb.v").to_string())
    }
}

// Shared Q16.16 phase-reduction coefficient for selected and standalone paths.
const SINCOS_K: i64 = 2_734_261_102;

#[cfg(test)]
mod tests;
