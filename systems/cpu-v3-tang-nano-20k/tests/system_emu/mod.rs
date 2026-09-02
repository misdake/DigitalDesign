//! Shared cycle-accurate full-system emulator for CpuV3 tests.
//!
//! Wires the core, instruction fetch queue, I-cache, D-cache and the memory
//! arbiter through their Rust emulators, and drives them against a cycle-faithful
//! model of the Tang Nano 20K SDRAM word port (ACTIVE / READ 4x64 / WRITE /
//! RECOVERY / periodic refresh). Used by `bench_emu.rs` for performance
//! benchmarks and by `system_cosim.rs` for emulator-vs-RTL co-simulation.
//!
//! This module is compiled into several integration test crates; not every
//! crate uses every entry point.
#![allow(dead_code)]

use cpu_v3::rcc_backend::{self, CompilerOptions};
use cpu_v3::{
    CpuV3Core, CpuV3CoreInput, CpuV3CoreOutput, CpuV3CoreOutputValue, CpuV3DataCache,
    CpuV3DataCacheInput, CpuV3DataCacheOutput, CpuV3InstructionFetchQueue,
    CpuV3InstructionFetchQueueInput, CpuV3InstructionFetchQueueOutput, CpuV3TwoWayCache,
    CpuV3TwoWayCacheInput, CpuV3TwoWayCacheOutput,
};
use cpu_v3_tang_nano_20k::{CpuV3MemoryArbiter, CpuV3MemoryArbiterInput, CpuV3MemoryArbiterOutput};
use digital_design_circuit::{build_circuit, Circuit, Wire, Wires};
use digital_design_hardware::{Module, ModuleIo};
use rcc::frontend::compile_program_named;
use std::collections::VecDeque;
use std::fs::{create_dir_all, File};
use std::io::{BufWriter, Write};
use std::path::Path;

const SDRAM_WORDS: usize = 0x10000;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SdramState {
    Idle,
    WriteCapture,
    WriteStage,
    ActiveReq,
    ActiveWait,
    OpReq,
    OpWait,
    CpuResponse,
    Recovery,
    RefreshReq,
    RefreshWait,
}

/// Cycle-faithful model of `display_sdram.v` for the CPU port only. Refresh is
/// due every 600 clocks; a line read costs ACTIVE + READ + four 64-bit beats + three
/// recovery clocks.
struct SdramModel {
    memory: Vec<u16>,
    state: SdramState,
    refresh_count: u16,
    pending_write: bool,
    pending_line: bool,
    pending_address: usize,
    pending_write_data: u64,
    line_write_buffer: [u64; 4],
    beat: u8,
    read_delay: u8,
    response_valid: bool,
    response_data: u64,
    response_last: bool,
    recovery_count: u8,
}

impl SdramModel {
    fn new(memory: Vec<u16>) -> Self {
        Self {
            memory,
            state: SdramState::Idle,
            refresh_count: 0,
            pending_write: false,
            pending_line: false,
            pending_address: 0,
            pending_write_data: 0,
            line_write_buffer: [0; 4],
            beat: 0,
            read_delay: 0,
            response_valid: false,
            response_data: 0,
            response_last: false,
            recovery_count: 0,
        }
    }

    /// Final SDRAM contents (meaningful after the post-halt D-cache clean).
    fn memory(&self) -> &[u16] {
        &self.memory
    }

    fn request_ready(&self) -> bool {
        self.state == SdramState::Idle && self.refresh_count < 600
    }

    fn clock(
        &mut self,
        request_valid: bool,
        write: bool,
        line: bool,
        address: u32,
        write_data: u64,
    ) {
        // Evaluate the refresh condition against the pre-edge counter, exactly
        // like `display_sdram.v` (refresh_due = refresh_count >= 600). The
        // counter is incremented afterwards and stops at 600, so a request
        // accepted on the last pre-refresh cycle is actually served.
        let refresh_due = self.refresh_count >= 600;
        let in_refresh_wait = self.state == SdramState::RefreshWait;

        match self.state {
            SdramState::Idle => {
                self.response_valid = false;
                if refresh_due {
                    self.state = SdramState::RefreshReq;
                } else if request_valid {
                    self.pending_write = write;
                    self.pending_line = line;
                    self.pending_address = address as usize;
                    self.pending_write_data = write_data;
                    if write && line {
                        self.line_write_buffer[0] = write_data;
                        self.beat = 1;
                        self.state = SdramState::WriteCapture;
                    } else {
                        self.state = SdramState::ActiveReq;
                    }
                }
            }
            SdramState::WriteCapture => {
                self.line_write_buffer[self.beat as usize] = write_data;
                if self.beat == 3 {
                    self.beat = 0;
                    self.state = SdramState::WriteStage;
                } else {
                    self.beat += 1;
                }
            }
            SdramState::WriteStage => {
                if self.beat == 3 {
                    self.beat = 0;
                    self.state = SdramState::ActiveReq;
                } else {
                    self.beat += 1;
                }
            }
            SdramState::ActiveReq => self.state = SdramState::ActiveWait,
            SdramState::ActiveWait => self.state = SdramState::OpReq,
            SdramState::OpReq => {
                if self.pending_write {
                    self.state = SdramState::OpWait;
                } else {
                    self.read_delay = 2;
                    self.beat = 0;
                    self.state = SdramState::OpWait;
                }
            }
            SdramState::OpWait => {
                if self.pending_write {
                    if self.pending_line {
                        for (beat, data) in self.line_write_buffer.iter().copied().enumerate() {
                            self.memory[self.pending_address + 4 * beat] = data as u16;
                            self.memory[self.pending_address + 4 * beat + 1] = (data >> 16) as u16;
                            self.memory[self.pending_address + 4 * beat + 2] = (data >> 32) as u16;
                            self.memory[self.pending_address + 4 * beat + 3] = (data >> 48) as u16;
                        }
                    } else {
                        self.memory[self.pending_address] = self.pending_write_data as u16;
                    }
                    self.response_valid = true;
                    self.response_data = 0;
                    self.response_last = true;
                    self.state = SdramState::CpuResponse;
                } else if self.read_delay != 0 {
                    self.read_delay -= 1;
                } else {
                    let address = self.pending_address + 4 * self.beat as usize;
                    self.response_data = u64::from(self.memory[address])
                        | u64::from(self.memory[address + 1]) << 16
                        | u64::from(self.memory[address + 2]) << 32
                        | u64::from(self.memory[address + 3]) << 48;
                    self.response_valid = true;
                    self.response_last = self.beat == 3;
                    if self.beat == 3 {
                        self.recovery_count = 0;
                        self.state = SdramState::Recovery;
                    } else {
                        self.beat += 1;
                    }
                }
            }
            SdramState::CpuResponse => {
                self.response_valid = false;
                self.recovery_count = 0;
                self.state = SdramState::Recovery;
            }
            SdramState::Recovery => {
                self.response_valid = false;
                if self.recovery_count == 3 {
                    self.state = SdramState::Idle;
                } else {
                    self.recovery_count += 1;
                }
            }
            SdramState::RefreshReq => self.state = SdramState::RefreshWait,
            SdramState::RefreshWait => {
                self.refresh_count = 0;
                self.state = SdramState::Idle;
            }
        }

        if !in_refresh_wait && !refresh_due {
            self.refresh_count += 1;
        }
    }
}

pub struct BenchResult {
    pub program_words: usize,
    pub cycles: usize,
    pub halt_signal: u16,
    pub retired_instructions: u32,
    pub retired_words: u32,
    pub prefetch_issued: u32,
    pub prefetch_useful: u32,
    pub prefetch_useless: u32,
    pub prefetch_dropped: u32,
    pub fetch_wait_cycles: usize,
    pub execute_cycles: usize,
    pub data_request_cycles: usize,
    pub data_response_cycles: usize,
    pub instruction_fetches: u32,
    pub icache_demand_requests: u32,
    pub data_requests: u32,
    pub icache_line_requests: u32,
    pub icache_demand_refills: u32,
    pub dcache_line_requests: u32,
    pub dcache_refills: u32,
    pub dcache_load_refills: u32,
    pub dcache_store_refills: u32,
    pub dcache_writebacks: u32,
    pub dcache_word_requests: u32,
    pub flush_cycles: u32,
    pub flush_writebacks: u32,
    pub refreshes: u32,
    pub redirect_count: u32,
    pub redirect_wait_cycles: u64,
    pub redirect_max_wait_cycles: u32,
    pub redirect_wait_histogram: [u32; 32],
    pub load_latency_cycles: u64,
    pub store_latency_cycles: u64,
    pub opcode_retired: [u32; 16],
    pub sdram_state_cycles: [u64; 11],
}

#[derive(Clone, Copy)]
struct PendingDataOp {
    is_write: bool,
    accept_cycle: usize,
}

struct TraceRecorder {
    control_flow: Option<BufWriter<File>>,
}

impl TraceRecorder {
    fn new(directory: Option<&Path>) -> Self {
        let control_flow = directory.map(|directory| {
            create_dir_all(directory).unwrap();
            let mut writer =
                BufWriter::new(File::create(directory.join("control-flow.csv")).unwrap());
            writeln!(
                writer,
                "origin,target,instruction,opcode,retired_cycle,target_fetch_cycle,wait_cycles"
            )
            .unwrap();
            writer
        });
        Self { control_flow }
    }

    fn redirect(
        &mut self,
        origin: u32,
        target: u32,
        instruction: u16,
        retired_cycle: usize,
        target_fetch_cycle: usize,
    ) {
        if let Some(writer) = &mut self.control_flow {
            writeln!(
                writer,
                "{origin:#010x},{target:#010x},{instruction:#06x},{},{retired_cycle},{target_fetch_cycle},{}",
                instruction >> 12,
                target_fetch_cycle - retired_cycle
            )
            .unwrap();
        }
    }
}

impl TraceRecorder {
    fn summary(&mut self, directory: Option<&Path>, result: &BenchResult) {
        let Some(directory) = directory else {
            return;
        };
        if let Some(writer) = &mut self.control_flow {
            writer.flush().unwrap();
        }
        let mut writer = BufWriter::new(File::create(directory.join("summary.txt")).unwrap());
        writeln!(writer, "program_words={}", result.program_words).unwrap();
        writeln!(writer, "cycles={}", result.cycles).unwrap();
        writeln!(
            writer,
            "retired_instructions={}",
            result.retired_instructions
        )
        .unwrap();
        writeln!(writer, "retired_words={}", result.retired_words).unwrap();
        writeln!(
            writer,
            "cycles_per_instruction={:.6}",
            result.cycles as f64 / f64::from(result.retired_instructions)
        )
        .unwrap();
        writeln!(
            writer,
            "cycles_per_retired_word={:.6}",
            result.cycles as f64 / f64::from(result.retired_words)
        )
        .unwrap();
        writeln!(
            writer,
            "fetch_wait_percent={:.3}",
            100.0 * result.fetch_wait_cycles as f64 / result.cycles as f64
        )
        .unwrap();
        writeln!(
            writer,
            "data_path_percent={:.3}",
            100.0 * (result.data_request_cycles + result.data_response_cycles) as f64
                / result.cycles as f64
        )
        .unwrap();
        writeln!(writer, "fetch_wait_cycles={}", result.fetch_wait_cycles).unwrap();
        writeln!(writer, "execute_cycles={}", result.execute_cycles).unwrap();
        writeln!(writer, "data_request_cycles={}", result.data_request_cycles).unwrap();
        writeln!(
            writer,
            "data_response_cycles={}",
            result.data_response_cycles
        )
        .unwrap();
        writeln!(writer, "instruction_fetches={}", result.instruction_fetches).unwrap();
        writeln!(
            writer,
            "icache_demand_requests={}",
            result.icache_demand_requests
        )
        .unwrap();
        writeln!(writer, "data_requests={}", result.data_requests).unwrap();
        writeln!(
            writer,
            "icache_line_requests={}",
            result.icache_line_requests
        )
        .unwrap();
        writeln!(
            writer,
            "icache_demand_refills={}",
            result.icache_demand_refills
        )
        .unwrap();
        writeln!(
            writer,
            "dcache_line_requests={}",
            result.dcache_line_requests
        )
        .unwrap();
        writeln!(writer, "dcache_refills={}", result.dcache_refills).unwrap();
        writeln!(writer, "dcache_load_refills={}", result.dcache_load_refills).unwrap();
        writeln!(
            writer,
            "dcache_store_refills={}",
            result.dcache_store_refills
        )
        .unwrap();
        writeln!(writer, "dcache_writebacks={}", result.dcache_writebacks).unwrap();
        writeln!(
            writer,
            "dcache_word_requests={}",
            result.dcache_word_requests
        )
        .unwrap();
        writeln!(writer, "flush_cycles={}", result.flush_cycles).unwrap();
        writeln!(writer, "flush_writebacks={}", result.flush_writebacks).unwrap();
        writeln!(writer, "refreshes={}", result.refreshes).unwrap();
        writeln!(writer, "redirect_count={}", result.redirect_count).unwrap();
        writeln!(
            writer,
            "redirect_wait_cycles={}",
            result.redirect_wait_cycles
        )
        .unwrap();
        writeln!(
            writer,
            "redirect_max_wait_cycles={}",
            result.redirect_max_wait_cycles
        )
        .unwrap();
        let redirect_average_wait_cycles = if result.redirect_count == 0 {
            0.0
        } else {
            result.redirect_wait_cycles as f64 / f64::from(result.redirect_count)
        };
        writeln!(
            writer,
            "redirect_average_wait_cycles={:.6}",
            redirect_average_wait_cycles
        )
        .unwrap();
        for (wait, count) in result.redirect_wait_histogram.iter().enumerate() {
            if *count != 0 {
                writeln!(writer, "redirect_wait_{wait}_count={count}").unwrap();
            }
        }
        writeln!(writer, "prefetch_issued={}", result.prefetch_issued).unwrap();
        writeln!(writer, "prefetch_useful={}", result.prefetch_useful).unwrap();
        writeln!(writer, "prefetch_useless={}", result.prefetch_useless).unwrap();
        writeln!(writer, "prefetch_dropped={}", result.prefetch_dropped).unwrap();
        writeln!(
            writer,
            "icache_demand_hit_percent={:.6}",
            if result.icache_demand_requests == 0 {
                0.0
            } else {
                100.0
                    * f64::from(
                        result
                            .icache_demand_requests
                            .saturating_sub(result.icache_demand_refills),
                    )
                    / f64::from(result.icache_demand_requests)
            }
        )
        .unwrap();
        writeln!(
            writer,
            "prefetch_precision_percent={:.6}",
            if result.prefetch_issued == 0 {
                0.0
            } else {
                100.0 * f64::from(result.prefetch_useful) / f64::from(result.prefetch_issued)
            }
        )
        .unwrap();
        writeln!(
            writer,
            "prefetch_coverage_percent={:.6}",
            if result.icache_demand_refills + result.prefetch_useful == 0 {
                0.0
            } else {
                100.0 * f64::from(result.prefetch_useful)
                    / f64::from(result.icache_demand_refills + result.prefetch_useful)
            }
        )
        .unwrap();
        let loads = result.opcode_retired[8];
        let stores = result.opcode_retired[9];
        writeln!(writer, "loads={loads}").unwrap();
        writeln!(writer, "stores={stores}").unwrap();
        writeln!(writer, "load_latency_cycles={}", result.load_latency_cycles).unwrap();
        writeln!(
            writer,
            "store_latency_cycles={}",
            result.store_latency_cycles
        )
        .unwrap();
        writeln!(
            writer,
            "load_average_wait_cycles={:.6}",
            if loads == 0 {
                0.0
            } else {
                result.load_latency_cycles as f64 / f64::from(loads)
            }
        )
        .unwrap();
        writeln!(
            writer,
            "store_average_wait_cycles={:.6}",
            if stores == 0 {
                0.0
            } else {
                result.store_latency_cycles as f64 / f64::from(stores)
            }
        )
        .unwrap();
        writeln!(
            writer,
            "dcache_load_hit_percent={:.6}",
            if loads == 0 {
                0.0
            } else {
                100.0 * f64::from(loads.saturating_sub(result.dcache_load_refills))
                    / f64::from(loads)
            }
        )
        .unwrap();
        writeln!(
            writer,
            "dcache_store_hit_percent={:.6}",
            if stores == 0 {
                0.0
            } else {
                100.0 * f64::from(stores.saturating_sub(result.dcache_store_refills))
                    / f64::from(stores)
            }
        )
        .unwrap();
        writeln!(
            writer,
            "dcache_access_hit_percent={:.6}",
            if result.data_requests == 0 {
                0.0
            } else {
                100.0 * f64::from(result.data_requests.saturating_sub(result.dcache_refills))
                    / f64::from(result.data_requests)
            }
        )
        .unwrap();
        for (opcode, count) in result.opcode_retired.iter().enumerate() {
            writeln!(writer, "opcode_{opcode:x}_retired={count}").unwrap();
        }
        let state_names = [
            "idle",
            "active_req",
            "active_wait",
            "op_req",
            "op_wait",
            "cpu_response",
            "recovery",
            "refresh_req",
            "refresh_wait",
            "write_capture",
            "write_stage",
        ];
        for (state, cycles) in result.sdram_state_cycles.iter().enumerate() {
            writeln!(writer, "sdram_{}_cycles={cycles}", state_names[state]).unwrap();
        }
    }
}

fn physical_pc(code_segment: u16, pc: u16) -> u32 {
    u32::from(code_segment) << 16 | u32::from(pc)
}

fn next_physical_word(address: u32) -> u32 {
    address & 0xffff_0000 | (address + 1) & 0xffff
}

fn sdram_state_index(state: SdramState) -> usize {
    match state {
        SdramState::Idle => 0,
        SdramState::ActiveReq => 1,
        SdramState::ActiveWait => 2,
        SdramState::OpReq => 3,
        SdramState::OpWait => 4,
        SdramState::CpuResponse => 5,
        SdramState::Recovery => 6,
        SdramState::RefreshReq => 7,
        SdramState::RefreshWait => 8,
        SdramState::WriteCapture => 9,
        SdramState::WriteStage => 10,
    }
}

fn set_bit(wire: Wire, value: bool, circuit: &mut Circuit) {
    wire.set(circuit, u8::from(value));
}

fn set_bits<const N: usize>(wires: Wires<N>, value: u64, circuit: &mut Circuit) {
    for i in 0..N {
        wires.wires[i].set(circuit, ((value >> i) & 1) as u8);
    }
}

/// Compiles an rcc CpuV3 source snippet to its loaded word image.
pub fn compile_cpu_v3_source(source: &str) -> Vec<u16> {
    let options = CompilerOptions::default();
    let program = compile_program_named("bench", source, &options, &mut |_| {
        Err("co-simulation program uses no modules".to_string())
    })
    .unwrap();
    rcc_backend::compile(program, &options, "main").words
}

/// Runs `words` from physical word zero (code and data share segment zero) until
/// the core halts, returning the cycle count and the I-cache prefetch counters.
pub fn run_benchmark(words: &[u16], maximum_cycles: usize) -> BenchResult {
    run_benchmark_profiled(words, maximum_cycles, None)
}

pub fn run_benchmark_profiled(
    words: &[u16],
    maximum_cycles: usize,
    trace_directory: Option<&Path>,
) -> BenchResult {
    run_benchmark_profiled_with_prefetch(words, maximum_cycles, trace_directory, true)
}

pub fn run_benchmark_profiled_with_prefetch(
    words: &[u16],
    maximum_cycles: usize,
    trace_directory: Option<&Path>,
    prefetch_enabled: bool,
) -> BenchResult {
    let mut memory = vec![0u16; SDRAM_WORDS];
    for (offset, word) in words.iter().copied().enumerate() {
        memory[offset] = word;
    }

    let (mut circuit, handles) = build_circuit(|| {
        let mut core_input = CpuV3CoreInput::allocate();
        let core_output = CpuV3CoreOutput::allocate();

        let mut fetch_input = CpuV3InstructionFetchQueueInput::allocate();
        let fetch_output = CpuV3InstructionFetchQueueOutput::allocate();

        let mut icache_input = CpuV3TwoWayCacheInput::allocate();
        let icache_output = CpuV3TwoWayCacheOutput::allocate();
        let disabled_prefetch_valid = icache_input.prefetch_request_valid;
        let disabled_prefetch_address = icache_input.prefetch_address;
        let disabled_prefetch_cancel = icache_input.prefetch_cancel;

        let mut dcache_input = CpuV3DataCacheInput::allocate();
        let dcache_output = CpuV3DataCacheOutput::allocate();

        let mut arbiter_input = CpuV3MemoryArbiterInput::allocate();
        let arbiter_output = CpuV3MemoryArbiterOutput::allocate();

        // core <-> fetch queue
        fetch_input.core_request_valid = core_output.instruction_request_valid;
        fetch_input.core_address = core_output.instruction_address;
        fetch_input.core_response_ready = core_output.instruction_response_ready;
        core_input.instruction_request_ready = fetch_output.core_request_ready;
        core_input.instruction_response_valid = fetch_output.core_response_valid;
        core_input.instruction_data = fetch_output.core_read_data;
        core_input.instruction_error = fetch_output.core_error;

        // fetch queue -> I-cache
        icache_input.cpu_request_valid = fetch_output.memory_request_valid;
        icache_input.cpu_address = fetch_output.memory_address;
        icache_input.cpu_response_ready = fetch_output.memory_response_ready;
        if prefetch_enabled {
            icache_input.prefetch_request_valid = fetch_output.prefetch_request_valid;
            icache_input.prefetch_address = fetch_output.prefetch_address;
            icache_input.prefetch_cancel = fetch_output.prefetch_cancel;
        }
        fetch_input.memory_request_ready = icache_output.cpu_request_ready;
        fetch_input.memory_response_valid = icache_output.cpu_response_valid;
        fetch_input.memory_read_data = icache_output.cpu_read_data;
        fetch_input.memory_error = icache_output.cpu_error;

        // core <-> D-cache
        dcache_input.cpu_request_valid = core_output.data_request_valid;
        dcache_input.cpu_write = core_output.data_write;
        dcache_input.cpu_address = core_output.data_address;
        dcache_input.cpu_write_data = core_output.data_write_data;
        dcache_input.cpu_response_ready = core_output.data_response_ready;
        core_input.data_request_ready = dcache_output.cpu_request_ready;
        core_input.data_response_valid = dcache_output.cpu_response_valid;
        core_input.data_read_data = dcache_output.cpu_read_data;
        core_input.data_error = dcache_output.cpu_error;

        // I-cache <-> arbiter
        arbiter_input.instruction_request_valid = icache_output.memory_request_valid;
        arbiter_input.instruction_address = icache_output.memory_address;
        arbiter_input.instruction_response_ready = icache_output.memory_response_ready;
        icache_input.memory_request_ready = arbiter_output.instruction_request_ready;
        icache_input.memory_response_valid = arbiter_output.instruction_response_valid;
        icache_input.memory_read_data = arbiter_output.instruction_read_data;
        icache_input.memory_error = arbiter_output.instruction_error;

        // D-cache <-> arbiter
        arbiter_input.data_request_valid = dcache_output.memory_request_valid;
        arbiter_input.data_write = dcache_output.memory_write;
        arbiter_input.data_line = dcache_output.memory_line;
        arbiter_input.data_address = dcache_output.memory_address;
        arbiter_input.data_write_data = dcache_output.memory_write_data;
        arbiter_input.data_response_ready = dcache_output.memory_response_ready;
        dcache_input.memory_request_ready = arbiter_output.data_request_ready;
        dcache_input.memory_response_valid = arbiter_output.data_response_valid;
        dcache_input.memory_read_data = arbiter_output.data_read_data;
        dcache_input.memory_error = arbiter_output.data_error;

        // Create the emulator externals after every wire is connected. The
        // order matches the combinational dependency (core, caches, arbiter).
        CpuV3Core::emu_connect(&core_input, &core_output);
        CpuV3InstructionFetchQueue::emu_connect(&fetch_input, &fetch_output);
        CpuV3TwoWayCache::emu_connect(&icache_input, &icache_output);
        CpuV3DataCache::emu_connect(&dcache_input, &dcache_output);
        CpuV3MemoryArbiter::emu_connect(&arbiter_input, &arbiter_output);

        (
            core_input,
            core_output,
            fetch_input,
            icache_input,
            dcache_input,
            arbiter_input,
            arbiter_output,
            icache_output,
            dcache_output,
            fetch_output,
            disabled_prefetch_valid,
            disabled_prefetch_address,
            disabled_prefetch_cancel,
        )
    });

    let (
        core_input,
        core_output,
        fetch_input,
        icache_input,
        dcache_input,
        arbiter_input,
        arbiter_output,
        icache_output,
        dcache_output,
        fetch_output,
        disabled_prefetch_valid,
        disabled_prefetch_address,
        disabled_prefetch_cancel,
    ) = handles;

    let mut sdram = SdramModel::new(memory);
    let mut trace = TraceRecorder::new(trace_directory);
    let mut previous_retired = 0u32;
    let mut retired_instructions = 0u32;
    let mut accepted_instructions = VecDeque::new();
    let mut pending_redirect = None;
    let mut fetch_wait_cycles = 0usize;
    let mut execute_cycles = 0usize;
    let mut data_request_cycles = 0usize;
    let mut data_response_cycles = 0usize;
    let mut load_latency_cycles = 0u64;
    let mut store_latency_cycles = 0u64;
    let mut pending_data_op: Option<PendingDataOp> = None;
    let mut instruction_fetches = 0u32;
    let mut icache_demand_requests = 0u32;
    let mut data_requests = 0u32;
    let mut icache_line_requests = 0u32;
    let mut dcache_line_requests = 0u32;
    let mut dcache_refills = 0u32;
    let mut dcache_load_refills = 0u32;
    let mut dcache_store_refills = 0u32;
    let mut dcache_writebacks = 0u32;
    let mut dcache_word_requests = 0u32;
    let mut flush_writebacks = 0u32;
    let mut refreshes = 0u32;
    let mut redirect_count = 0u32;
    let mut redirect_wait_cycles = 0u64;
    let mut redirect_max_wait_cycles = 0u32;
    let mut redirect_wait_histogram = [0u32; 32];
    let mut opcode_retired = [0u32; 16];
    let mut sdram_state_cycles = [0u64; 11];
    let mut halt_at = None;
    let mut halt_signal = 0u16;
    let mut flush_request = false;

    // Constant external inputs (device reads zero, DMA idle, no flush/invalidate).
    set_bits(core_input.device_read_data, 0, &mut circuit);
    set_bit(core_input.hold, false, &mut circuit);
    set_bit(fetch_input.flush, false, &mut circuit);
    set_bit(icache_input.invalidate_all, false, &mut circuit);
    set_bit(dcache_input.invalidate_all, false, &mut circuit);
    set_bit(dcache_input.clean_all, false, &mut circuit);
    set_bit(arbiter_input.dma_request_valid, false, &mut circuit);
    set_bit(arbiter_input.dma_write, false, &mut circuit);
    set_bit(arbiter_input.dma_response_ready, false, &mut circuit);
    set_bits(arbiter_input.dma_address, 0, &mut circuit);
    set_bits(arbiter_input.dma_write_data, 0, &mut circuit);
    set_bit(arbiter_input.memory_error, false, &mut circuit);
    set_bit(disabled_prefetch_valid, false, &mut circuit);
    set_bits(disabled_prefetch_address, 0, &mut circuit);
    set_bit(disabled_prefetch_cancel, false, &mut circuit);

    for cycle in 0..maximum_cycles + 100_000 {
        if halt_at.is_none() && cycle >= maximum_cycles {
            panic!("benchmark exceeded {maximum_cycles} cycles");
        }
        let reset = cycle < 2;
        set_bit(core_input.reset, reset, &mut circuit);
        set_bit(fetch_input.reset, reset, &mut circuit);
        set_bit(icache_input.reset, reset, &mut circuit);
        set_bit(dcache_input.reset, reset, &mut circuit);
        set_bit(arbiter_input.reset, reset, &mut circuit);
        set_bit(dcache_input.clean_all, flush_request, &mut circuit);

        set_bit(
            arbiter_input.memory_request_ready,
            sdram.request_ready(),
            &mut circuit,
        );
        set_bit(
            arbiter_input.memory_response_valid,
            sdram.response_valid,
            &mut circuit,
        );
        set_bits(
            arbiter_input.memory_read_data,
            sdram.response_data,
            &mut circuit,
        );
        set_bit(
            arbiter_input.memory_response_last,
            sdram.response_last,
            &mut circuit,
        );

        // The composed emulator externals form ready/valid paths in both
        // directions. Re-evaluate to a fixed point approximation so a
        // cache-response fall-through reaches fetch and core in the same
        // architectural cycle, matching continuous RTL combinational logic.
        const COMBINATIONAL_SETTLE_PASSES: usize = 6;
        for _ in 0..COMBINATIONAL_SETTLE_PASSES {
            circuit.execute_gates();
        }

        let core = core_output.sample(&circuit);
        let arb = arbiter_output.sample(&circuit);
        let icache = icache_output.sample(&circuit);
        let dcache = dcache_output.sample(&circuit);
        let fetch = fetch_output.sample(&circuit);

        sdram_state_cycles[sdram_state_index(sdram.state)] += 1;

        if !reset && halt_at.is_none() {
            let retired = core.retired_words as u32;
            if retired != previous_retired {
                retired_instructions = retired_instructions.wrapping_add(1);
                let retired_words = retired.wrapping_sub(previous_retired);
                let mut retired_instruction = None;
                for _ in 0..retired_words {
                    let entry = accepted_instructions
                        .pop_front()
                        .expect("retired word was never accepted by the core frontend");
                    let (_, instruction) = entry;
                    opcode_retired[usize::from(instruction >> 12)] =
                        opcode_retired[usize::from(instruction >> 12)].wrapping_add(1);
                    retired_instruction = Some(entry);
                }
                if let Some((origin, instruction)) = retired_instruction {
                    let target = physical_pc(core.code_segment as u16, core.pc as u16);
                    let opcode = instruction >> 12;
                    let function = (instruction >> 8) & 0xfu16;
                    let can_redirect = opcode == 0xbu16
                        || (opcode == 0xeu16 && matches!(function, 4u16 | 5u16 | 15u16));
                    if can_redirect && target != next_physical_word(origin) {
                        redirect_count = redirect_count.wrapping_add(1);
                        pending_redirect = Some((origin, target, instruction, cycle));
                    }
                }
                previous_retired = retired;
            }

            let mut interface_active = false;
            if fetch.memory_request_valid && icache.cpu_request_ready {
                icache_demand_requests = icache_demand_requests.wrapping_add(1);
            }
            if core.instruction_request_valid {
                interface_active = true;
                if fetch.core_request_ready {
                    instruction_fetches = instruction_fetches.wrapping_add(1);
                    let address = physical_pc(core.code_segment as u16, core.pc as u16);
                    accepted_instructions.push_back((address, fetch.core_read_data as u16));
                    if let Some((origin, target, instruction, retired_cycle)) =
                        pending_redirect.take()
                    {
                        debug_assert_eq!(address, target);
                        let wait = (cycle - retired_cycle) as u32;
                        redirect_wait_cycles += u64::from(wait);
                        redirect_max_wait_cycles = redirect_max_wait_cycles.max(wait);
                        let bucket = usize::try_from(wait.min(31)).unwrap();
                        redirect_wait_histogram[bucket] =
                            redirect_wait_histogram[bucket].wrapping_add(1);
                        trace.redirect(origin, target, instruction, retired_cycle, cycle);
                    }
                } else {
                    fetch_wait_cycles += 1;
                }
            }
            // Stage 11 may issue its buffered store while fetching the next
            // instruction. Count the independent interfaces independently;
            // an else-if chain silently drops every overlapped store.
            if core.data_request_valid {
                interface_active = true;
                data_request_cycles += 1;
                if dcache.cpu_request_ready {
                    data_requests = data_requests.wrapping_add(1);
                    pending_data_op = Some(PendingDataOp {
                        is_write: core.data_write,
                        accept_cycle: cycle,
                    });
                }
            }
            if core.data_response_ready {
                interface_active = true;
                data_response_cycles += 1;
            }
            if !interface_active && !core.halted && !core.fault {
                execute_cycles += 1;
            }

            if let Some(op) = pending_data_op {
                if !core.data_request_valid && core.data_response_ready && dcache.cpu_response_valid
                {
                    let latency = (cycle - op.accept_cycle) as u64;
                    if op.is_write {
                        store_latency_cycles = store_latency_cycles.wrapping_add(latency);
                    } else {
                        load_latency_cycles = load_latency_cycles.wrapping_add(latency);
                    }
                    pending_data_op = None;
                }
            }
        }

        if arb.memory_request_valid && sdram.request_ready() {
            if halt_at.is_none() {
                if dcache.memory_request_valid {
                    if arb.memory_line {
                        dcache_line_requests = dcache_line_requests.wrapping_add(1);
                        if arb.memory_write {
                            dcache_writebacks = dcache_writebacks.wrapping_add(1);
                        } else {
                            dcache_refills = dcache_refills.wrapping_add(1);
                            if pending_data_op.is_some_and(|op| op.is_write) {
                                dcache_store_refills = dcache_store_refills.wrapping_add(1);
                            } else {
                                dcache_load_refills = dcache_load_refills.wrapping_add(1);
                            }
                        }
                    } else {
                        dcache_word_requests = dcache_word_requests.wrapping_add(1);
                    }
                } else if icache.memory_request_valid {
                    icache_line_requests = icache_line_requests.wrapping_add(1);
                }
            } else if dcache.memory_request_valid && arb.memory_line && arb.memory_write {
                flush_writebacks = flush_writebacks.wrapping_add(1);
            }
        }
        if sdram.state == SdramState::RefreshWait {
            refreshes = refreshes.wrapping_add(1);
        }

        circuit.clock_tick();
        flush_request = false;

        sdram.clock(
            arb.memory_request_valid,
            arb.memory_write,
            arb.memory_line,
            arb.memory_address as u32,
            arb.memory_write_data,
        );

        if core.fault {
            panic!(
                "CPU faulted with code {} at {:#06x}",
                core.fault_code, core.fault_pc
            );
        }

        if halt_at.is_none() && core.halted {
            halt_at = Some(cycle + 1);
            halt_signal = core.halt_signal as u16;
            flush_request = true;
        } else if let Some(main_cycles) = halt_at {
            if dcache.maintenance_error {
                panic!("D-cache flush failed after benchmark completion");
            }
            if dcache.maintenance_done {
                let icache_demand_refills =
                    icache_line_requests.saturating_sub(icache.prefetch_issued as u32);
                let result = BenchResult {
                    program_words: words.len(),
                    cycles: main_cycles,
                    halt_signal,
                    retired_instructions,
                    retired_words: core.retired_words as u32,
                    prefetch_issued: icache.prefetch_issued as u32,
                    prefetch_useful: icache.prefetch_useful as u32,
                    prefetch_useless: icache.prefetch_useless as u32,
                    prefetch_dropped: icache.prefetch_dropped as u32,
                    fetch_wait_cycles,
                    execute_cycles,
                    data_request_cycles,
                    data_response_cycles,
                    instruction_fetches,
                    icache_demand_requests,
                    data_requests,
                    icache_line_requests,
                    icache_demand_refills,
                    dcache_line_requests,
                    dcache_refills,
                    dcache_load_refills,
                    dcache_store_refills,
                    dcache_writebacks,
                    dcache_word_requests,
                    flush_cycles: u32::try_from(cycle + 1 - main_cycles).unwrap(),
                    flush_writebacks,
                    refreshes,
                    redirect_count,
                    redirect_wait_cycles,
                    redirect_max_wait_cycles,
                    redirect_wait_histogram,
                    load_latency_cycles,
                    store_latency_cycles,
                    opcode_retired,
                    sdram_state_cycles,
                };
                trace.summary(trace_directory, &result);
                return result;
            }
        }
    }
    panic!("D-cache flush exceeded 100000 cycles");
}

/// One cycle of observable core-port state, mirroring the core-level co-sim's
/// `CoreCosimOut` field set. Every field is directly observable at the
/// `CpuV3Core` ports in the full-system wiring.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SystemCosimOut {
    pub pc: u16,
    pub code_segment: u16,
    pub data_segment: u16,
    pub retired_words: u32,
    pub halted: bool,
    pub halt_signal: u16,
    pub fault: bool,
    pub fault_code: u8,
    pub fault_pc: u16,
    pub instruction_request_valid: bool,
    pub instruction_address: u32,
    pub instruction_response_ready: bool,
    pub data_request_valid: bool,
    pub data_write: bool,
    pub data_address: u32,
    pub data_write_data: u16,
    pub data_response_ready: bool,
}

impl SystemCosimOut {
    pub fn equal_core(&self, other: &Self) -> bool {
        self == other
    }
}

impl From<&CpuV3CoreOutputValue> for SystemCosimOut {
    fn from(value: &CpuV3CoreOutputValue) -> Self {
        Self {
            pc: value.pc as u16,
            code_segment: value.code_segment as u16,
            data_segment: value.data_segment as u16,
            retired_words: value.retired_words as u32,
            halted: value.halted,
            halt_signal: value.halt_signal as u16,
            fault: value.fault,
            fault_code: value.fault_code as u8,
            fault_pc: value.fault_pc as u16,
            instruction_request_valid: value.instruction_request_valid,
            instruction_address: value.instruction_address as u32,
            instruction_response_ready: value.instruction_response_ready,
            data_request_valid: value.data_request_valid,
            data_write: value.data_write,
            data_address: value.data_address as u32,
            data_write_data: value.data_write_data as u16,
            data_response_ready: value.data_response_ready,
        }
    }
}

/// Full-system trace result: one entry per cycle from the first instruction
/// request until halt, plus the SDRAM contents after the post-halt D-cache
/// clean has written every dirty line back.
pub struct SystemTrace {
    pub cycles: Vec<SystemCosimOut>,
    pub halt_signal: u16,
    pub halted: bool,
    pub memory: Vec<u16>,
}

/// Runs `words` from physical word zero through the full system model (core,
/// fetch queue, I-cache, D-cache, arbiter, SDRAM model), recording one
/// `SystemCosimOut` per cycle from the first cycle where
/// `instruction_request_valid` is observed until the halted cycle (inclusive).
/// Afterwards pulses the D-cache `clean_all` input for one cycle, waits for
/// `maintenance_done`, and captures the final SDRAM contents. Panics on fault
/// or when `maximum_cycles` is exceeded before halt.
pub fn run_system_trace(words: &[u16], maximum_cycles: usize) -> SystemTrace {
    let mut memory = vec![0u16; SDRAM_WORDS];
    for (offset, word) in words.iter().copied().enumerate() {
        memory[offset] = word;
    }

    let (mut circuit, handles) = build_circuit(|| {
        let mut core_input = CpuV3CoreInput::allocate();
        let core_output = CpuV3CoreOutput::allocate();

        let mut fetch_input = CpuV3InstructionFetchQueueInput::allocate();
        let fetch_output = CpuV3InstructionFetchQueueOutput::allocate();

        let mut icache_input = CpuV3TwoWayCacheInput::allocate();
        let icache_output = CpuV3TwoWayCacheOutput::allocate();

        let mut dcache_input = CpuV3DataCacheInput::allocate();
        let dcache_output = CpuV3DataCacheOutput::allocate();

        let mut arbiter_input = CpuV3MemoryArbiterInput::allocate();
        let arbiter_output = CpuV3MemoryArbiterOutput::allocate();

        // core <-> fetch queue
        fetch_input.core_request_valid = core_output.instruction_request_valid;
        fetch_input.core_address = core_output.instruction_address;
        fetch_input.core_response_ready = core_output.instruction_response_ready;
        core_input.instruction_request_ready = fetch_output.core_request_ready;
        core_input.instruction_response_valid = fetch_output.core_response_valid;
        core_input.instruction_data = fetch_output.core_read_data;
        core_input.instruction_error = fetch_output.core_error;

        // fetch queue -> I-cache (prefetch wired through)
        icache_input.cpu_request_valid = fetch_output.memory_request_valid;
        icache_input.cpu_address = fetch_output.memory_address;
        icache_input.cpu_response_ready = fetch_output.memory_response_ready;
        icache_input.prefetch_request_valid = fetch_output.prefetch_request_valid;
        icache_input.prefetch_address = fetch_output.prefetch_address;
        icache_input.prefetch_cancel = fetch_output.prefetch_cancel;
        fetch_input.memory_request_ready = icache_output.cpu_request_ready;
        fetch_input.memory_response_valid = icache_output.cpu_response_valid;
        fetch_input.memory_read_data = icache_output.cpu_read_data;
        fetch_input.memory_error = icache_output.cpu_error;

        // core <-> D-cache
        dcache_input.cpu_request_valid = core_output.data_request_valid;
        dcache_input.cpu_write = core_output.data_write;
        dcache_input.cpu_address = core_output.data_address;
        dcache_input.cpu_write_data = core_output.data_write_data;
        dcache_input.cpu_response_ready = core_output.data_response_ready;
        core_input.data_request_ready = dcache_output.cpu_request_ready;
        core_input.data_response_valid = dcache_output.cpu_response_valid;
        core_input.data_read_data = dcache_output.cpu_read_data;
        core_input.data_error = dcache_output.cpu_error;

        // I-cache <-> arbiter
        arbiter_input.instruction_request_valid = icache_output.memory_request_valid;
        arbiter_input.instruction_address = icache_output.memory_address;
        arbiter_input.instruction_response_ready = icache_output.memory_response_ready;
        icache_input.memory_request_ready = arbiter_output.instruction_request_ready;
        icache_input.memory_response_valid = arbiter_output.instruction_response_valid;
        icache_input.memory_read_data = arbiter_output.instruction_read_data;
        icache_input.memory_error = arbiter_output.instruction_error;

        // D-cache <-> arbiter
        arbiter_input.data_request_valid = dcache_output.memory_request_valid;
        arbiter_input.data_write = dcache_output.memory_write;
        arbiter_input.data_line = dcache_output.memory_line;
        arbiter_input.data_address = dcache_output.memory_address;
        arbiter_input.data_write_data = dcache_output.memory_write_data;
        arbiter_input.data_response_ready = dcache_output.memory_response_ready;
        dcache_input.memory_request_ready = arbiter_output.data_request_ready;
        dcache_input.memory_response_valid = arbiter_output.data_response_valid;
        dcache_input.memory_read_data = arbiter_output.data_read_data;
        dcache_input.memory_error = arbiter_output.data_error;

        // Create the emulator externals after every wire is connected. The
        // order matches the combinational dependency (core, caches, arbiter).
        CpuV3Core::emu_connect(&core_input, &core_output);
        CpuV3InstructionFetchQueue::emu_connect(&fetch_input, &fetch_output);
        CpuV3TwoWayCache::emu_connect(&icache_input, &icache_output);
        CpuV3DataCache::emu_connect(&dcache_input, &dcache_output);
        CpuV3MemoryArbiter::emu_connect(&arbiter_input, &arbiter_output);

        (
            core_input,
            core_output,
            fetch_input,
            icache_input,
            dcache_input,
            arbiter_input,
            arbiter_output,
            dcache_output,
        )
    });

    let (
        core_input,
        core_output,
        fetch_input,
        icache_input,
        dcache_input,
        arbiter_input,
        arbiter_output,
        dcache_output,
    ) = handles;

    let mut sdram = SdramModel::new(memory);
    let mut cycles: Vec<SystemCosimOut> = Vec::new();
    let mut started = false;
    let mut halt_at = None;
    let mut halt_signal = 0u16;
    let mut flush_request = false;

    // Constant external inputs (device reads zero, DMA idle, no flush/invalidate).
    set_bits(core_input.device_read_data, 0, &mut circuit);
    set_bit(core_input.hold, false, &mut circuit);
    set_bit(fetch_input.flush, false, &mut circuit);
    set_bit(icache_input.invalidate_all, false, &mut circuit);
    set_bit(dcache_input.invalidate_all, false, &mut circuit);
    set_bit(dcache_input.clean_all, false, &mut circuit);
    set_bit(arbiter_input.dma_request_valid, false, &mut circuit);
    set_bit(arbiter_input.dma_write, false, &mut circuit);
    set_bit(arbiter_input.dma_response_ready, false, &mut circuit);
    set_bits(arbiter_input.dma_address, 0, &mut circuit);
    set_bits(arbiter_input.dma_write_data, 0, &mut circuit);
    set_bit(arbiter_input.memory_error, false, &mut circuit);

    for cycle in 0..maximum_cycles + 100_000 {
        if halt_at.is_none() && cycle >= maximum_cycles {
            panic!("system trace exceeded {maximum_cycles} cycles");
        }
        let reset = cycle < 2;
        set_bit(core_input.reset, reset, &mut circuit);
        set_bit(fetch_input.reset, reset, &mut circuit);
        set_bit(icache_input.reset, reset, &mut circuit);
        set_bit(dcache_input.reset, reset, &mut circuit);
        set_bit(arbiter_input.reset, reset, &mut circuit);
        set_bit(dcache_input.clean_all, flush_request, &mut circuit);

        set_bit(
            arbiter_input.memory_request_ready,
            sdram.request_ready(),
            &mut circuit,
        );
        set_bit(
            arbiter_input.memory_response_valid,
            sdram.response_valid,
            &mut circuit,
        );
        set_bits(
            arbiter_input.memory_read_data,
            sdram.response_data,
            &mut circuit,
        );
        set_bit(
            arbiter_input.memory_response_last,
            sdram.response_last,
            &mut circuit,
        );

        // The composed emulator externals form ready/valid paths in both
        // directions. Re-evaluate to a fixed point approximation so a
        // cache-response fall-through reaches fetch and core in the same
        // architectural cycle, matching continuous RTL combinational logic.
        const COMBINATIONAL_SETTLE_PASSES: usize = 6;
        for _ in 0..COMBINATIONAL_SETTLE_PASSES {
            circuit.execute_gates();
        }

        let core = core_output.sample(&circuit);
        let arb = arbiter_output.sample(&circuit);
        let dcache = dcache_output.sample(&circuit);

        // Record from the first instruction request (the same "started" rule
        // as the core-level co-sim) through the halted cycle, inclusive.
        if !reset && halt_at.is_none() && (started || core.instruction_request_valid) {
            started = true;
            cycles.push(SystemCosimOut::from(&core));
        }

        circuit.clock_tick();
        flush_request = false;

        sdram.clock(
            arb.memory_request_valid,
            arb.memory_write,
            arb.memory_line,
            arb.memory_address as u32,
            arb.memory_write_data,
        );

        if core.fault {
            panic!(
                "CPU faulted with code {} at {:#06x}",
                core.fault_code, core.fault_pc
            );
        }

        if halt_at.is_none() && core.halted {
            halt_at = Some(cycle + 1);
            halt_signal = core.halt_signal as u16;
            flush_request = true;
        } else if halt_at.is_some() {
            if dcache.maintenance_error {
                panic!("D-cache flush failed after system trace completion");
            }
            if dcache.maintenance_done {
                return SystemTrace {
                    cycles,
                    halt_signal,
                    halted: true,
                    memory: sdram.memory().to_vec(),
                };
            }
        }
    }
    panic!("D-cache flush exceeded 100000 cycles");
}
