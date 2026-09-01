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

const QUEUE_DEPTH: usize = 4;

/// Cycle-accurate model of the four-entry fetch queue. Mirrors the register
/// set and per-cycle behavior of `cpu_v3_instruction_fetch_queue.v`.
#[derive(Clone, Default)]
pub struct CpuV3InstructionFetchQueueState {
    stream_valid: bool,
    epoch: bool,
    expected_core_address: u32,
    next_memory_address: u32,
    queue_data: [u16; QUEUE_DEPTH],
    queue_error: [bool; QUEUE_DEPTH],
    queue_address: [u32; QUEUE_DEPTH],
    queue_head: u8,
    queue_tail: u8,
    queue_count: u8,
    metadata_epoch: [bool; QUEUE_DEPTH],
    metadata_address: [u32; QUEUE_DEPTH],
    metadata_head: u8,
    metadata_tail: u8,
    metadata_count: u8,
}

impl Module for CpuV3InstructionFetchQueue {
    type Input = CpuV3InstructionFetchQueueInput;
    type Output = CpuV3InstructionFetchQueueOutput;
    type EmuState = CpuV3InstructionFetchQueueState;

    const USES_MAIN_CLOCK: bool = true;

    fn create_emu(_input: &Self::Input, _output: &Self::Output) -> Self::EmuState {
        CpuV3InstructionFetchQueueState::default()
    }

    fn execute_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        input: &Self::Input,
        output: &Self::Output,
    ) {
        let input = input.sample(circuit);
        let core_address = input.core_address as u32;
        let core_address_matches =
            state.stream_valid && core_address == state.expected_core_address;
        let queue_head_matches = state.queue_count != 0
            && state.queue_address[usize::from(state.queue_head)] == core_address;
        let restart = input.core_request_valid
            && (!core_address_matches || (state.queue_count != 0 && !queue_head_matches));
        let response_is_current = state.metadata_count != 0
            && state.metadata_epoch[usize::from(state.metadata_head)] == state.epoch;
        let core_response_valid =
            input.core_request_valid && core_address_matches && queue_head_matches;
        let core_pop = input.core_request_valid && input.core_response_ready && core_response_valid;
        let reserved_words = state.queue_count + state.metadata_count;
        let memory_response_ready = state.metadata_count != 0
            && (!response_is_current || state.queue_count < QUEUE_DEPTH as u8 || core_pop);
        let memory_request_valid =
            state.stream_valid && !input.flush && !restart && reserved_words < QUEUE_DEPTH as u8;

        output.drive(
            circuit,
            &CpuV3InstructionFetchQueueOutputValue {
                core_request_ready: core_response_valid && input.core_response_ready,
                core_response_valid,
                core_read_data: u64::from(state.queue_data[usize::from(state.queue_head)]),
                core_error: state.queue_error[usize::from(state.queue_head)],
                memory_request_valid,
                memory_address: u64::from(state.next_memory_address),
                memory_response_ready,
            },
        );
    }

    fn clock_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        input: &Self::Input,
        _output: &Self::Output,
    ) {
        let input = input.sample(circuit);
        let core_address = input.core_address as u32;
        let core_address_matches =
            state.stream_valid && core_address == state.expected_core_address;
        let queue_head_matches = state.queue_count != 0
            && state.queue_address[usize::from(state.queue_head)] == core_address;
        let restart = input.core_request_valid
            && (!core_address_matches || (state.queue_count != 0 && !queue_head_matches));
        let response_is_current = state.metadata_count != 0
            && state.metadata_epoch[usize::from(state.metadata_head)] == state.epoch;
        let core_response_valid =
            input.core_request_valid && core_address_matches && queue_head_matches;
        let core_pop = input.core_request_valid && input.core_response_ready && core_response_valid;
        let reserved_words = state.queue_count + state.metadata_count;
        let memory_response_ready = state.metadata_count != 0
            && (!response_is_current || state.queue_count < QUEUE_DEPTH as u8 || core_pop);
        let memory_response_fire = input.memory_response_valid && memory_response_ready;
        let memory_request_valid =
            state.stream_valid && !input.flush && !restart && reserved_words < QUEUE_DEPTH as u8;
        let memory_request_fire = memory_request_valid && input.memory_request_ready;
        let enqueue_response =
            memory_response_fire && response_is_current && !input.flush && !restart;

        if input.reset {
            *state = CpuV3InstructionFetchQueueState::default();
            return;
        }

        let issue_address = state.next_memory_address;
        let issue_epoch = state.epoch;
        let next_issue_address = (issue_address & 0xffff_0000) | ((issue_address + 1) & 0xffff);
        let next_expected_address = (state.expected_core_address & 0xffff_0000)
            | ((state.expected_core_address + 1) & 0xffff);
        if input.flush || restart {
            state.epoch = !state.epoch;
            state.queue_head = 0;
            state.queue_tail = 0;
            state.queue_count = 0;
            if input.core_request_valid {
                state.stream_valid = true;
                state.expected_core_address = core_address;
                state.next_memory_address = core_address;
            } else {
                state.stream_valid = false;
            }
        } else {
            if core_pop {
                state.queue_head = (state.queue_head + 1) & (QUEUE_DEPTH as u8 - 1);
                state.expected_core_address = next_expected_address;
            }
            if enqueue_response {
                state.queue_data[usize::from(state.queue_tail)] = input.memory_read_data as u16;
                state.queue_error[usize::from(state.queue_tail)] = input.memory_error;
                state.queue_address[usize::from(state.queue_tail)] =
                    state.metadata_address[usize::from(state.metadata_head)];
                state.queue_tail = (state.queue_tail + 1) & (QUEUE_DEPTH as u8 - 1);
            }
            match (enqueue_response, core_pop) {
                (true, false) => state.queue_count += 1,
                (false, true) => state.queue_count -= 1,
                _ => {}
            }
            if memory_request_fire {
                state.next_memory_address = next_issue_address;
            }
        }

        if memory_request_fire {
            state.metadata_epoch[usize::from(state.metadata_tail)] = issue_epoch;
            state.metadata_address[usize::from(state.metadata_tail)] = issue_address;
            state.metadata_tail = (state.metadata_tail + 1) & (QUEUE_DEPTH as u8 - 1);
        }
        if memory_response_fire {
            state.metadata_head = (state.metadata_head + 1) & (QUEUE_DEPTH as u8 - 1);
        }
        match (memory_request_fire, memory_response_fire) {
            (true, false) => state.metadata_count += 1,
            (false, true) => state.metadata_count -= 1,
            _ => {}
        }
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
