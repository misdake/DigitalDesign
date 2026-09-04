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
    pub prefetch_request_valid: Wire,
    pub prefetch_address: Wires<32>,
    pub prefetch_cancel: Wire,
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
        let response_bypass = input.core_request_valid
            && core_address_matches
            && state.queue_count == 0
            && input.memory_response_valid
            && response_is_current
            && state.metadata_address[usize::from(state.metadata_head)] == core_address
            && !input.flush
            && !restart;
        let core_response_valid = input.core_request_valid
            && core_address_matches
            && (queue_head_matches || response_bypass);
        let core_pop = input.core_request_valid && input.core_response_ready && core_response_valid;
        let reserved_words = state.queue_count + state.metadata_count;
        let memory_response_ready = state.metadata_count != 0
            && (!response_is_current || state.queue_count < QUEUE_DEPTH as u8 || core_pop);
        let memory_response_fire = input.memory_response_valid && memory_response_ready;
        let redirect_slot_available =
            state.metadata_count < QUEUE_DEPTH as u8 || memory_response_fire;
        let memory_request_valid = !input.flush
            && ((restart && redirect_slot_available)
                || (!restart && state.stream_valid && reserved_words < QUEUE_DEPTH as u8));

        let prefetch_address =
            (core_address & 0xffff_0000) | ((((core_address >> 4) & 0x0fff) + 1) & 0x0fff) << 4;
        output.drive(
            circuit,
            &CpuV3InstructionFetchQueueOutputValue {
                core_request_ready: core_response_valid && input.core_response_ready,
                core_response_valid,
                core_read_data: u64::from(if response_bypass {
                    input.memory_read_data as u16
                } else {
                    state.queue_data[usize::from(state.queue_head)]
                }),
                core_error: if response_bypass {
                    input.memory_error
                } else {
                    state.queue_error[usize::from(state.queue_head)]
                },
                memory_request_valid,
                memory_address: u64::from(if restart {
                    core_address
                } else {
                    state.next_memory_address
                }),
                memory_response_ready,
                prefetch_request_valid: core_pop && core_address & 0xf == 10,
                prefetch_address: u64::from(prefetch_address),
                prefetch_cancel: input.flush || restart,
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
        let response_bypass = input.core_request_valid
            && core_address_matches
            && state.queue_count == 0
            && input.memory_response_valid
            && response_is_current
            && state.metadata_address[usize::from(state.metadata_head)] == core_address
            && !input.flush
            && !restart;
        let core_response_valid = input.core_request_valid
            && core_address_matches
            && (queue_head_matches || response_bypass);
        let core_pop = input.core_request_valid && input.core_response_ready && core_response_valid;
        let queue_pop = core_pop && !response_bypass;
        let bypass_pop = core_pop && response_bypass;
        let reserved_words = state.queue_count + state.metadata_count;
        let memory_response_ready = state.metadata_count != 0
            && (!response_is_current || state.queue_count < QUEUE_DEPTH as u8 || core_pop);
        let memory_response_fire = input.memory_response_valid && memory_response_ready;
        let redirect_slot_available =
            state.metadata_count < QUEUE_DEPTH as u8 || memory_response_fire;
        let memory_request_valid = !input.flush
            && ((restart && redirect_slot_available)
                || (!restart && state.stream_valid && reserved_words < QUEUE_DEPTH as u8));
        let memory_request_fire = memory_request_valid && input.memory_request_ready;
        let enqueue_response =
            memory_response_fire && response_is_current && !input.flush && !restart && !bypass_pop;

        if input.reset {
            *state = CpuV3InstructionFetchQueueState::default();
            return;
        }

        let issue_address = if restart {
            core_address
        } else {
            state.next_memory_address
        };
        let issue_epoch = if restart { !state.epoch } else { state.epoch };
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
                state.next_memory_address = if memory_request_fire {
                    next_issue_address
                } else {
                    core_address
                };
            } else {
                state.stream_valid = false;
            }
        } else {
            if core_pop {
                if queue_pop {
                    state.queue_head = (state.queue_head + 1) & (QUEUE_DEPTH as u8 - 1);
                }
                state.expected_core_address = next_expected_address;
            }
            if enqueue_response {
                state.queue_data[usize::from(state.queue_tail)] = input.memory_read_data as u16;
                state.queue_error[usize::from(state.queue_tail)] = input.memory_error;
                state.queue_address[usize::from(state.queue_tail)] =
                    state.metadata_address[usize::from(state.metadata_head)];
                state.queue_tail = (state.queue_tail + 1) & (QUEUE_DEPTH as u8 - 1);
            }
            match (enqueue_response, queue_pop) {
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
    use digital_design_circuit::{build_circuit, Circuit, Wires};
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
    fn sequential_alu_stream_reaches_one_cycle_throughput() {
        digital_design_hardware::verify_verilog_with_iverilog::<FetchPipelineProbe>().unwrap();
    }

    // ---- emulator vs RTL co-simulation ----

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    struct QueueOut {
        core_request_ready: bool,
        core_response_valid: bool,
        core_read_data: u16,
        core_error: bool,
        memory_request_valid: bool,
        memory_address: u32,
        memory_response_ready: bool,
        prefetch_request_valid: bool,
        prefetch_address: u32,
        prefetch_cancel: bool,
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    struct QueueIn {
        flush: bool,
        core_request_valid: bool,
        core_address: u32,
        core_response_ready: bool,
        memory_response_valid: bool,
        memory_read_data: u16,
        memory_error: bool,
    }

    #[allow(clippy::too_many_arguments)]
    fn queue_cosim_step(
        circuit: &mut Circuit,
        input: &CpuV3InstructionFetchQueueInput,
        output: &CpuV3InstructionFetchQueueOutput,
        memory_pending: &mut Option<u32>,
        flush: bool,
        core_request_valid: bool,
        core_address: u32,
        core_response_ready: bool,
        trace: &mut Vec<(QueueIn, QueueOut)>,
    ) -> QueueOut {
        let memory_response = memory_pending.take();
        input.drive(
            circuit,
            &CpuV3InstructionFetchQueueInputValue {
                reset: false,
                flush,
                core_request_valid,
                core_address: u64::from(core_address),
                core_response_ready,
                memory_request_ready: true,
                memory_response_valid: memory_response.is_some(),
                memory_read_data: u64::from(word_pattern(memory_response.unwrap_or(0))),
                memory_error: false,
            },
        );
        circuit.execute_gates();
        let value = output.sample(circuit);
        if value.memory_request_valid {
            *memory_pending = Some(value.memory_address as u32);
        }
        let out = QueueOut {
            core_request_ready: value.core_request_ready,
            core_response_valid: value.core_response_valid,
            core_read_data: value.core_read_data as u16,
            core_error: value.core_error,
            memory_request_valid: value.memory_request_valid,
            memory_address: value.memory_address as u32,
            memory_response_ready: value.memory_response_ready,
            prefetch_request_valid: value.prefetch_request_valid,
            prefetch_address: value.prefetch_address as u32,
            prefetch_cancel: value.prefetch_cancel,
        };
        let cin = QueueIn {
            flush,
            core_request_valid,
            core_address,
            core_response_ready,
            memory_response_valid: memory_response.is_some(),
            memory_read_data: word_pattern(memory_response.unwrap_or(0)),
            memory_error: false,
        };
        trace.push((cin, out));
        circuit.clock_tick();
        out
    }

    fn word_pattern(address: u32) -> u16 {
        (0x6000 ^ (address & 0xffff)) as u16
    }

    /// Parses a decimal field, mapping Verilog unknown (`x`/`z`) bits to zero.
    fn parse_num(value: &str) -> u32 {
        value.parse().unwrap_or(0)
    }

    fn queue_consume(
        circuit: &mut Circuit,
        input: &CpuV3InstructionFetchQueueInput,
        output: &CpuV3InstructionFetchQueueOutput,
        memory_pending: &mut Option<u32>,
        address: u32,
        trace: &mut Vec<(QueueIn, QueueOut)>,
    ) {
        for _ in 0..50 {
            let out = queue_cosim_step(
                circuit,
                input,
                output,
                memory_pending,
                false,
                true,
                address,
                true,
                trace,
            );
            if out.core_response_valid {
                return;
            }
        }
        panic!("fetch queue did not deliver word {address:#x}");
    }

    fn run_queue_trace() -> Vec<(QueueIn, QueueOut)> {
        let (mut circuit, (input, output)) = build_circuit(|| {
            let input = CpuV3InstructionFetchQueueInput::allocate();
            let output = CpuV3InstructionFetchQueue::emu(&input);
            (input, output)
        });
        // Reset: two cycles.
        input.drive(
            &mut circuit,
            &CpuV3InstructionFetchQueueInputValue {
                reset: true,
                flush: false,
                core_request_valid: false,
                core_address: 0,
                core_response_ready: false,
                memory_request_ready: false,
                memory_response_valid: false,
                memory_read_data: 0,
                memory_error: false,
            },
        );
        circuit.execute_gates();
        circuit.clock_tick();
        circuit.execute_gates();
        circuit.clock_tick();

        let mut memory_pending = None;
        let mut trace = Vec::new();

        queue_consume(
            &mut circuit,
            &input,
            &output,
            &mut memory_pending,
            0x1000,
            &mut trace,
        );
        queue_consume(
            &mut circuit,
            &input,
            &output,
            &mut memory_pending,
            0x1001,
            &mut trace,
        );
        queue_consume(
            &mut circuit,
            &input,
            &output,
            &mut memory_pending,
            0x1002,
            &mut trace,
        );
        // Redirect to a distant address (restart).
        queue_consume(
            &mut circuit,
            &input,
            &output,
            &mut memory_pending,
            0x2000,
            &mut trace,
        );
        queue_consume(
            &mut circuit,
            &input,
            &output,
            &mut memory_pending,
            0x2001,
            &mut trace,
        );
        // Flush toggles the epoch.
        queue_cosim_step(
            &mut circuit,
            &input,
            &output,
            &mut memory_pending,
            true,
            false,
            0,
            false,
            &mut trace,
        );
        queue_consume(
            &mut circuit,
            &input,
            &output,
            &mut memory_pending,
            0x3000,
            &mut trace,
        );

        trace
    }

    fn generate_queue_tb(trace: &[(QueueIn, QueueOut)], module_name: &str) -> String {
        let mut t = format!(
            "module tb;\n\
             reg clk = 0;\n\
             reg reset, flush, core_request_valid, core_response_ready;\n\
             reg [31:0] core_address;\n\
             reg memory_request_ready, memory_response_valid, memory_error;\n\
             reg [15:0] memory_read_data;\n\
             wire core_request_ready, core_response_valid, core_error;\n\
             wire [15:0] core_read_data;\n\
             wire memory_request_valid, memory_response_ready;\n\
             wire [31:0] memory_address;\n\
             wire prefetch_request_valid, prefetch_cancel;\n\
             wire [31:0] prefetch_address;\n\n\
             {module_name} dut(.*);\n\n\
             always #5 clk = ~clk;\n\n\
             initial begin\n\
                 reset = 1; flush = 0; core_request_valid = 0; core_address = 0; core_response_ready = 0;\n\
                 memory_request_ready = 0; memory_response_valid = 0; memory_read_data = 0; memory_error = 0;\n\
                 repeat (2) @(posedge clk);\n\
                 reset = 0;\n\
                 @(posedge clk);\n\
                 @(negedge clk);\n",
        );
        for (i, (cin, _)) in trace.iter().enumerate() {
            t.push_str(&format!(
                "    // cycle {i}\n\
                 flush = 1'b{f}; core_request_valid = 1'b{crv}; core_address = 32'h{ca:08x}; core_response_ready = 1'b{crr};\n\
                 memory_request_ready = 1'b1; memory_response_valid = 1'b{mrv}; memory_read_data = 16'h{mrd:04x}; memory_error = 1'b{me};\n\
                 #1;\n\
                 $display(\"OUT %0d %0d %0d %0d %0d %0d %0d %0d %0d %0d %0d\", {i}, core_request_ready, core_response_valid, core_read_data, core_error, memory_request_valid, memory_address, memory_response_ready, prefetch_request_valid, prefetch_address, prefetch_cancel);\n\
                 @(posedge clk);\n\
                 @(negedge clk);\n",
                f = u8::from(cin.flush),
                crv = u8::from(cin.core_request_valid),
                ca = cin.core_address,
                crr = u8::from(cin.core_response_ready),
                mrv = u8::from(cin.memory_response_valid),
                mrd = cin.memory_read_data,
                me = u8::from(cin.memory_error),
            ));
        }
        t.push_str(&format!(
            "    $display(\"TRACE_END\");\n    $finish;\nend\n\n\
             initial begin\n    repeat ({}) @(posedge clk);\n    $display(\"TIMEOUT\");\n    $finish(1);\nend\nendmodule\n",
            trace.len() * 5 + 500
        ));
        t
    }

    fn run_queue_iverilog(tb: &str) -> Vec<QueueOut> {
        let directory = std::env::temp_dir().join(format!("fetch-cosim-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join("modules.v"),
            CpuV3InstructionFetchQueue::verilog_source().unwrap(),
        )
        .unwrap();
        std::fs::write(directory.join("tb.v"), tb).unwrap();
        let iverilog = std::env::var_os("IVERILOG_EXE").unwrap_or_else(|| "iverilog".into());
        let vvp = std::env::var_os("VVP_EXE").unwrap_or_else(|| "vvp".into());
        let output_path = directory.join("sim.vvp");
        let compile = std::process::Command::new(&iverilog)
            .current_dir(&directory)
            .args(["-g2005", "-s", "tb", "-o"])
            .arg(&output_path)
            .arg(directory.join("modules.v"))
            .arg(directory.join("tb.v"))
            .output()
            .unwrap();
        assert!(
            compile.status.success(),
            "iverilog compile failed:\n{}",
            String::from_utf8_lossy(&compile.stderr)
        );
        let simulation = std::process::Command::new(&vvp)
            .current_dir(&directory)
            .arg(&output_path)
            .output()
            .unwrap();
        assert!(
            simulation.status.success(),
            "vvp failed:\n{}",
            String::from_utf8_lossy(&simulation.stderr)
        );
        let stdout = String::from_utf8_lossy(&simulation.stdout);
        let mut outputs = Vec::new();
        for line in stdout.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("OUT ") {
                let fields: Vec<&str> = rest.split_whitespace().collect();
                assert_eq!(fields.len(), 11, "unexpected OUT line: {line}");
                outputs.push(QueueOut {
                    core_request_ready: fields[1] == "1",
                    core_response_valid: fields[2] == "1",
                    core_read_data: parse_num(fields[3]) as u16,
                    core_error: fields[4] == "1",
                    memory_request_valid: fields[5] == "1",
                    memory_address: fields[6].parse().unwrap(),
                    memory_response_ready: fields[7] == "1",
                    prefetch_request_valid: fields[8] == "1",
                    prefetch_address: parse_num(fields[9]),
                    prefetch_cancel: fields[10] == "1",
                });
            } else if line == "TRACE_END" {
                break;
            }
        }
        std::fs::remove_dir_all(&directory).ok();
        outputs
    }

    #[test]
    #[ignore = "explicit emulator-vs-Icarus co-simulation of the fetch queue"]
    fn emu_matches_rtl_verilog() {
        let trace = run_queue_trace();
        let module_name = CpuV3InstructionFetchQueue::verilog_identity().module_name();
        let tb = generate_queue_tb(&trace, &module_name);
        let rtl = run_queue_iverilog(&tb);
        assert_eq!(rtl.len(), trace.len(), "cycle count mismatch");
        for (i, ((_, expected), actual)) in trace.iter().zip(&rtl).enumerate() {
            // `core_read_data`/`core_error` are only meaningful when a response
            // is being delivered; the RTL leaves the queue RAM unknown (x)
            // before its first enqueue, so those fields are don't-care here.
            let mut expected = *expected;
            let mut actual = *actual;
            if !expected.core_response_valid {
                expected.core_read_data = 0;
                expected.core_error = false;
                actual.core_read_data = 0;
                actual.core_error = false;
            }
            assert_eq!(actual, expected, "emu/RTL output mismatch at cycle {i}");
        }
    }
}
