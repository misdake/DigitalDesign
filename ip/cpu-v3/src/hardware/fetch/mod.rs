//! Four-entry instruction fetch queue with a resolved-target, two-word BTC.

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

/// Reserves four fetched/outstanding words. Per-slot current bits invalidate
/// late responses across arbitrarily many redirects, without epoch wraparound.
#[derive(Hardware)]
#[hardware(namespace = "components/cpu/cpu_v3")]
pub struct CpuV3InstructionFetchQueue;

include!(concat!(env!("OUT_DIR"), "/fetch_config.rs"));
const QUEUE_DEPTH: usize = 4;
const BTC_STORAGE: usize = if CPU_V3_BTC_ENTRIES == 0 {
    1
} else {
    CPU_V3_BTC_ENTRIES
};

/// Model-only diagnostics, intentionally absent from the hardware ports and
/// frozen benchmark schema. A lookup/hit counts once when a replay starts,
/// even if the core initially backpressures it.
#[derive(Clone, Copy, Default, Debug)]
pub struct CpuV3BtcStatistics {
    pub lookups: u64,
    pub hits: u64,
    pub installed: u64,
    pub cancelled_fills: u64,
    pub accepted_words: u64,
    pub aborted_replays: u64,
    /// Core-request cycles waiting after both BTC words, until the first
    /// ordinary word is accepted or the stream is redirected/flushed.
    pub continuation_wait_cycles: u64,
}

#[derive(Clone, Copy, Default)]
struct BtcEntry {
    valid: bool,
    tag: u32,
    words: [u16; 2],
    rank: u8,
}

/// Cycle-accurate register model of `cpu_v3_instruction_fetch_queue.v`.
#[derive(Clone, Default)]
pub struct CpuV3InstructionFetchQueueState {
    stream_valid: bool,
    expected_core_address: u32,
    next_memory_address: u32,
    queue_data: [u16; QUEUE_DEPTH],
    queue_error: [bool; QUEUE_DEPTH],
    queue_address: [u32; QUEUE_DEPTH],
    queue_head: u8,
    queue_tail: u8,
    queue_count: u8,
    metadata_current: [bool; QUEUE_DEPTH],
    metadata_address: [u32; QUEUE_DEPTH],
    metadata_head: u8,
    metadata_tail: u8,
    metadata_count: u8,
    btc: [BtcEntry; BTC_STORAGE],
    replay_entry: usize,
    replay_remaining: u8,
    fill_phase: u8,
    fill_tag: u32,
    fill_word: u16,
    statistics: CpuV3BtcStatistics,
    tracking_continuation: bool,
}

fn next_word(address: u32, count: u32) -> u32 {
    (address & 0xffff_0000) | (address.wrapping_add(count) & 0xffff)
}

struct FetchSignals {
    output: CpuV3InstructionFetchQueueOutputValue,
    restart: bool,
    hit: Option<usize>,
    btc_response: bool,
    core_pop: bool,
    queue_pop: bool,
    request_fire: bool,
    response_fire: bool,
    enqueue: bool,
}

impl CpuV3InstructionFetchQueueState {
    pub fn btc_statistics(&self) -> CpuV3BtcStatistics {
        self.statistics
    }

    fn signals(&self, input: &CpuV3InstructionFetchQueueInputValue) -> FetchSignals {
        let address = input.core_address as u32;
        let address_matches = self.stream_valid && address == self.expected_core_address;
        let head_matches =
            self.queue_count != 0 && self.queue_address[self.queue_head as usize] == address;
        let replay = self.replay_remaining != 0;
        let restart = input.core_request_valid
            && (!address_matches || (!replay && self.queue_count != 0 && !head_matches));
        let hit = if restart && address >> 22 == 0 && !input.flush && !input.reset {
            self.btc[..CPU_V3_BTC_ENTRIES]
                .iter()
                .position(|e| e.valid && e.tag == address)
        } else {
            None
        };
        let btc_response = !input.reset
            && !input.flush
            && input.core_request_valid
            && (hit.is_some() || (!restart && address_matches && replay));
        let current =
            self.metadata_count != 0 && self.metadata_current[self.metadata_head as usize];
        let bypass = !input.reset
            && !input.flush
            && !restart
            && !btc_response
            && input.core_request_valid
            && address_matches
            && self.queue_count == 0
            && input.memory_response_valid
            && current
            && self.metadata_address[self.metadata_head as usize] == address;
        let valid = !input.reset
            && !input.flush
            && input.core_request_valid
            && (btc_response || (!restart && address_matches && (head_matches || bypass)));
        let core_pop = valid && input.core_response_ready;
        let queue_pop = core_pop && !btc_response && !bypass;
        let response_ready = !input.reset
            && self.metadata_count != 0
            && (input.flush
                || restart
                || !current
                || self.queue_count < QUEUE_DEPTH as u8
                || queue_pop);
        let response_fire = input.memory_response_valid && response_ready;
        let request_valid = !input.reset
            && !input.flush
            && ((restart && (self.metadata_count < QUEUE_DEPTH as u8 || response_fire))
                || (!restart
                    && self.stream_valid
                    && self.queue_count + self.metadata_count < QUEUE_DEPTH as u8));
        let issue_address = if restart {
            next_word(address, if hit.is_some() { 2 } else { 0 })
        } else {
            self.next_memory_address
        };
        let (data, error) = if btc_response {
            let index = hit.unwrap_or(self.replay_entry);
            let word = if hit.is_some() || self.replay_remaining == 2 {
                0
            } else {
                1
            };
            (self.btc[index].words[word], false)
        } else if bypass {
            (input.memory_read_data as u16, input.memory_error)
        } else {
            (
                self.queue_data[self.queue_head as usize],
                self.queue_error[self.queue_head as usize],
            )
        };
        FetchSignals {
            output: CpuV3InstructionFetchQueueOutputValue {
                core_request_ready: core_pop,
                core_response_valid: valid,
                core_read_data: data.into(),
                core_error: error,
                memory_request_valid: request_valid,
                memory_address: issue_address.into(),
                memory_response_ready: response_ready,
            },
            restart,
            hit,
            btc_response,
            core_pop,
            queue_pop,
            request_fire: request_valid && input.memory_request_ready,
            response_fire,
            enqueue: response_fire && current && !input.flush && !restart && !(core_pop && bypass),
        }
    }

    fn touch(&mut self, index: usize, installing: bool) {
        let old_rank = self.btc[index].rank;
        for (i, entry) in self.btc[..CPU_V3_BTC_ENTRIES].iter_mut().enumerate() {
            if i == index {
                entry.rank = 0;
            } else if entry.valid && (installing || entry.rank < old_rank) {
                entry.rank = (entry.rank + 1).min((CPU_V3_BTC_ENTRIES as u8).saturating_sub(1));
            }
        }
    }

    fn clock(&mut self, input: &CpuV3InstructionFetchQueueInputValue) {
        if input.reset {
            *self = Self::default();
            return;
        }
        let sig = self.signals(input);
        let address = input.core_address as u32;
        let issue_address = sig.output.memory_address as u32;
        let response_address = self.metadata_address[self.metadata_head as usize];
        if (input.flush || sig.restart) && self.fill_phase != 0 {
            self.statistics.cancelled_fills += 1;
        }
        if (input.flush || sig.restart) && self.replay_remaining != 0 {
            self.statistics.aborted_replays += 1;
        }
        if sig.restart && !input.flush && CPU_V3_BTC_ENTRIES != 0 && address >> 22 == 0 {
            self.statistics.lookups += 1;
            self.statistics.hits += u64::from(sig.hit.is_some());
        }
        if !input.flush && !sig.restart && self.tracking_continuation {
            if input.core_request_valid
                && !sig.output.core_response_valid
                && self.replay_remaining == 0
            {
                self.statistics.continuation_wait_cycles += 1;
            }
            if sig.core_pop && !sig.btc_response {
                self.tracking_continuation = false;
            }
        }
        if sig.core_pop && sig.btc_response {
            self.statistics.accepted_words += 1;
        }

        if input.flush {
            for entry in &mut self.btc {
                entry.valid = false;
            }
            self.replay_remaining = 0;
            self.fill_phase = 0;
            self.tracking_continuation = false;
        } else if sig.restart {
            self.replay_remaining = if sig.hit.is_some() {
                2 - u8::from(sig.core_pop)
            } else {
                0
            };
            self.tracking_continuation = sig.hit.is_some();
            self.fill_phase = if CPU_V3_BTC_ENTRIES != 0 && address >> 22 == 0 && sig.hit.is_none()
            {
                1
            } else {
                0
            };
            self.fill_tag = address & 0x3f_ffff;
            if let Some(index) = sig.hit {
                self.replay_entry = index;
                if sig.core_pop {
                    self.touch(index, false);
                }
            }
        } else if sig.core_pop {
            if sig.btc_response {
                if self.replay_remaining == 2 {
                    self.touch(self.replay_entry, false);
                }
                self.replay_remaining -= 1;
            }
            if self.fill_phase != 0 {
                if sig.output.core_error {
                    self.fill_phase = 0;
                    self.statistics.cancelled_fills += 1;
                } else if self.fill_phase == 1 && address == self.fill_tag {
                    self.fill_word = sig.output.core_read_data as u16;
                    self.fill_phase = 2;
                } else if self.fill_phase == 2 && address == next_word(self.fill_tag, 1) {
                    let index = self.btc[..CPU_V3_BTC_ENTRIES]
                        .iter()
                        .position(|e| !e.valid)
                        .unwrap_or_else(|| {
                            self.btc[..CPU_V3_BTC_ENTRIES]
                                .iter()
                                .position(|e| {
                                    e.rank == (CPU_V3_BTC_ENTRIES as u8).saturating_sub(1)
                                })
                                .unwrap()
                        });
                    self.touch(index, true);
                    self.btc[index] = BtcEntry {
                        valid: true,
                        tag: self.fill_tag,
                        words: [self.fill_word, sig.output.core_read_data as u16],
                        rank: 0,
                    };
                    self.fill_phase = 0;
                    self.statistics.installed += 1;
                }
            }
        }

        if input.flush || sig.restart {
            self.metadata_current.fill(false);
            self.queue_head = 0;
            self.queue_tail = 0;
            self.queue_count = 0;
            self.stream_valid = input.core_request_valid;
            if input.core_request_valid {
                self.expected_core_address = next_word(address, u32::from(sig.core_pop));
                self.next_memory_address = if input.flush {
                    address
                } else {
                    next_word(issue_address, u32::from(sig.request_fire))
                };
            }
        } else {
            if sig.core_pop {
                self.expected_core_address = next_word(self.expected_core_address, 1);
            }
            if sig.queue_pop {
                self.queue_head = (self.queue_head + 1) & 3;
            }
            if sig.enqueue {
                self.queue_data[self.queue_tail as usize] = input.memory_read_data as u16;
                self.queue_error[self.queue_tail as usize] = input.memory_error;
                self.queue_address[self.queue_tail as usize] = response_address;
                self.queue_tail = (self.queue_tail + 1) & 3;
            }
            self.queue_count = self.queue_count + u8::from(sig.enqueue) - u8::from(sig.queue_pop);
            if sig.request_fire {
                self.next_memory_address = next_word(issue_address, 1);
            }
        }
        // A new request wins if a full FIFO drains and reuses the same slot.
        if sig.response_fire {
            self.metadata_current[self.metadata_head as usize] = false;
            self.metadata_head = (self.metadata_head + 1) & 3;
        }
        if sig.request_fire {
            self.metadata_current[self.metadata_tail as usize] = true;
            self.metadata_address[self.metadata_tail as usize] = issue_address;
            self.metadata_tail = (self.metadata_tail + 1) & 3;
        }
        self.metadata_count =
            self.metadata_count + u8::from(sig.request_fire) - u8::from(sig.response_fire);
    }
}

impl Module for CpuV3InstructionFetchQueue {
    type Input = CpuV3InstructionFetchQueueInput;
    type Output = CpuV3InstructionFetchQueueOutput;
    type EmuState = CpuV3InstructionFetchQueueState;
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
        output.drive(circuit, &state.signals(&input.sample(circuit)).output);
    }
    fn clock_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        input: &Self::Input,
        _output: &Self::Output,
    ) {
        state.clock(&input.sample(circuit));
    }
    fn verilog_source() -> Option<String> {
        Some(
            include_str!("cpu_v3_instruction_fetch_queue.v")
                .replace("__BTC_ENTRIES__", &CPU_V3_BTC_ENTRIES.to_string()),
        )
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
        // Flush clears request ownership and the BTC.
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
             wire [31:0] memory_address;\n\n\
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
                 $display(\"OUT %0d %0d %0d %0d %0d %0d %0d %0d\", {i}, core_request_ready, core_response_valid, core_read_data, core_error, memory_request_valid, memory_address, memory_response_ready);\n\
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
                assert_eq!(fields.len(), 8, "unexpected OUT line: {line}");
                outputs.push(QueueOut {
                    core_request_ready: fields[1] == "1",
                    core_response_valid: fields[2] == "1",
                    core_read_data: parse_num(fields[3]) as u16,
                    core_error: fields[4] == "1",
                    memory_request_valid: fields[5] == "1",
                    memory_address: fields[6].parse().unwrap(),
                    memory_response_ready: fields[7] == "1",
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

#[cfg(test)]
#[path = "fetch_btc_tests.rs"]
mod btc_tests;
