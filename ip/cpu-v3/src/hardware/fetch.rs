//! Four-entry speculative instruction fetch queue for the blocking CpuV3 core.

use digital_design_circuit::{CircuitWires, Wire, Wires};
use digital_design_hardware::{Hardware, Module, ModuleIo};

#[derive(Clone, ModuleIo)]
pub struct CpuV3InstructionFetchQueueInput {
    pub reset: Wire,
    pub flush: Wire,
    pub core_request_valid: Wire,
    pub core_address: Wires<32>,
    pub core_response_ready: Wire,
    pub memory_request_ready: Wire,
    pub memory_response_valid: Wire,
    pub memory_read_data: Wires<16>,
    pub memory_error: Wire,
}

#[derive(Clone, ModuleIo)]
pub struct CpuV3InstructionFetchQueueOutput {
    pub core_request_ready: Wire,
    pub core_response_valid: Wire,
    pub core_read_data: Wires<16>,
    pub core_error: Wire,
    pub memory_request_valid: Wire,
    pub memory_address: Wires<32>,
    pub memory_response_ready: Wire,
}

/// Keeps four fetched or outstanding words reserved and tags every downstream
/// request with an internal epoch. Redirects and invalidation toggle the epoch,
/// so responses already in flight are drained but never reach the core.
#[derive(Hardware)]
#[hardware(namespace = "components/cpu/cpu_v3")]
pub struct CpuV3InstructionFetchQueue;

impl Module for CpuV3InstructionFetchQueue {
    type Input = CpuV3InstructionFetchQueueInput;
    type Output = CpuV3InstructionFetchQueueOutput;
    type EmuState = ();

    const USES_MAIN_CLOCK: bool = true;
    const EMU_AVAILABLE: bool = false;

    fn execute_emu(
        _state: &mut Self::EmuState,
        _circuit: &mut CircuitWires,
        _input: &Self::Input,
        _output: &Self::Output,
    ) {
        panic!("CpuV3 instruction fetch queue is Verilog-only")
    }

    fn verilog_source() -> Option<String> {
        Some(include_str!("cpu_v3_instruction_fetch_queue.v").to_string())
    }

    fn verilog_testbench() -> Option<String> {
        Some(include_str!("cpu_v3_instruction_fetch_queue_tb.v").to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CpuV3Core;
    use digital_design_circuit::Wires;
    use digital_design_hardware::{HardwareIdentity, VerilogDependency};

    #[derive(Clone, ModuleIo)]
    struct FetchPipelineProbeInput {
        reset: Wire,
    }

    #[derive(Clone, ModuleIo)]
    struct FetchPipelineProbeOutput {
        halted: Wire,
        fault: Wire,
        halt_signal: Wires<16>,
        retired_words: Wires<32>,
    }

    #[derive(Hardware)]
    #[hardware(namespace = "tests/cpu_v3")]
    struct FetchPipelineProbe;

    impl Module for FetchPipelineProbe {
        type Input = FetchPipelineProbeInput;
        type Output = FetchPipelineProbeOutput;
        type EmuState = ();

        const USES_MAIN_CLOCK: bool = true;
        const EMU_AVAILABLE: bool = false;

        fn execute_emu(
            _state: &mut Self::EmuState,
            _circuit: &mut CircuitWires,
            _input: &Self::Input,
            _output: &Self::Output,
        ) {
            panic!("fetch pipeline probe is Verilog-only")
        }

        fn verilog_source() -> Option<String> {
            Some(
                include_str!("cpu_v3_fetch_pipeline_probe.v")
                    .replace("__CPU_CORE__", &CpuV3Core::verilog_identity().module_name())
                    .replace(
                        "__FETCH_QUEUE__",
                        &CpuV3InstructionFetchQueue::verilog_identity().module_name(),
                    ),
            )
        }

        fn verilog_dependencies() -> Vec<VerilogDependency> {
            vec![
                VerilogDependency::new::<CpuV3Core>("u_core"),
                VerilogDependency::new::<CpuV3InstructionFetchQueue>("u_fetch"),
            ]
        }

        fn verilog_testbench() -> Option<String> {
            Some(include_str!("cpu_v3_fetch_pipeline_probe_tb.v").to_string())
        }
    }

    #[test]
    #[ignore = "explicit external simulation of the pipelined instruction fetch queue"]
    fn verify_verilog_with_iverilog() {
        digital_design_hardware::verify_verilog_with_iverilog::<CpuV3InstructionFetchQueue>()
            .unwrap();
    }

    #[test]
    #[ignore = "explicit cycle-count simulation of the complete fetch frontend"]
    fn sequential_alu_stream_reaches_two_cycle_throughput() {
        digital_design_hardware::verify_verilog_with_iverilog::<FetchPipelineProbe>().unwrap();
    }
}
