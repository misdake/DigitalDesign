//! Shared multiplier boundary, physical claim and standalone pipeline peer.
use super::*;

#[derive(Clone, ModuleIo)]
pub struct CpuV3FpuMulPipeInput {
    pub abort: Wire,
    pub in_valid: Wire,
    pub in_a: Wires<36>,
    pub in_b: Wires<36>,
    pub in_tag: Wires<9>,
}

#[derive(Clone, ModuleIo)]
pub struct CpuV3FpuMulPipeOutput {
    pub out_valid: Wire,
    pub out_product: Wires<64>,
    pub out_tag: Wires<9>,
    pub ordinary_pending: Wire,
}
/// Shared FPU v2 36x36 multiply pipeline (one MULT36X36 = four 18x18 DSP
/// lanes): two register stages, tag-carrying FIFO. The multiply and dot
/// paths share it because the core serializes instructions. Emu lives in the
/// unit top; this leaf is verified through its Verilog testbench.
pub struct CpuV3FpuMulPipe;

impl HardwareIdentity for CpuV3FpuMulPipe {
    const TARGET_RESOURCE_LEAF: bool = true;

    fn verilog_identity() -> VerilogIdentity {
        VerilogIdentity::new("CpuV3FpuMulPipe").namespace(["components", "cpu", "cpu_v3"])
    }
}

impl Module for CpuV3FpuMulPipe {
    type Input = CpuV3FpuMulPipeInput;
    type Output = CpuV3FpuMulPipeOutput;
    type EmuState = ();

    const USES_MAIN_CLOCK: bool = true;
    const EMU_AVAILABLE: bool = false;

    fn target_resources() -> Vec<TargetResourceRequest> {
        vec![TargetResourceRequest::new(
            digital_design_hardware::resources::components::DspMultipliers::new(4),
        )]
    }

    fn verilog_source() -> Option<String> {
        Some(include_str!("cpu_v3_fpu_mul_pipe.v").to_string())
    }

    fn verilog_testbench() -> Option<String> {
        Some(include_str!("cpu_v3_fpu_mul_pipe_tb.v").to_string())
    }
}
