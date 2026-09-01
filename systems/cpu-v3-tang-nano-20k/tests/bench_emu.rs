//! Cycle-accurate full-system emulator for CpuV3 performance benchmarks.
//!
//! Wires the core, instruction fetch queue, I-cache, D-cache and the memory
//! arbiter through their Rust emulators, and drives them against a cycle-faithful
//! model of the Tang Nano 20K SDRAM word port (ACTIVE / READ 4x64 / WRITE /
//! RECOVERY / periodic refresh). It reports the total cycle count and the
//! cache and control-flow probes, so a workload can be profiled without
//! Icarus or hardware.

use cpu_v3::{
    alu, fpu, fpu_unary, halt, jump_relative, load_immediate16, nop, AluOp, CpuV3Core,
    CpuV3CoreInput, CpuV3CoreOutput, CpuV3DataCache, CpuV3DataCacheInput, CpuV3DataCacheOutput,
    CpuV3InstructionFetchQueue, CpuV3InstructionFetchQueueInput, CpuV3InstructionFetchQueueOutput,
    CpuV3TwoWayCache, CpuV3TwoWayCacheInput, CpuV3TwoWayCacheOutput, FpuOp, FpuUnaryOp,
};
use cpu_v3_tang_nano_20k::{CpuV3MemoryArbiter, CpuV3MemoryArbiterInput, CpuV3MemoryArbiterOutput};
use digital_design_circuit::{build_circuit, Circuit, Wire, Wires};
use digital_design_hardware::{Module, ModuleIo};
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
    pub data_requests: u32,
    pub icache_line_requests: u32,
    pub dcache_line_requests: u32,
    pub dcache_refills: u32,
    pub dcache_load_refills: u32,
    pub dcache_store_refills: u32,
    pub dcache_writebacks: u32,
    pub dcache_word_requests: u32,
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
        writeln!(writer, "retired_words={}", result.retired_words).unwrap();
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
        writeln!(writer, "data_requests={}", result.data_requests).unwrap();
        writeln!(
            writer,
            "icache_line_requests={}",
            result.icache_line_requests
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
    let mut last_fetched = None;
    let mut pending_redirect = None;
    let mut fetch_wait_cycles = 0usize;
    let mut execute_cycles = 0usize;
    let mut data_request_cycles = 0usize;
    let mut data_response_cycles = 0usize;
    let mut load_latency_cycles = 0u64;
    let mut store_latency_cycles = 0u64;
    let mut pending_data_op: Option<PendingDataOp> = None;
    let mut instruction_fetches = 0u32;
    let mut data_requests = 0u32;
    let mut icache_line_requests = 0u32;
    let mut dcache_line_requests = 0u32;
    let mut dcache_refills = 0u32;
    let mut dcache_load_refills = 0u32;
    let mut dcache_store_refills = 0u32;
    let mut dcache_writebacks = 0u32;
    let mut dcache_word_requests = 0u32;
    let mut refreshes = 0u32;
    let mut redirect_count = 0u32;
    let mut redirect_wait_cycles = 0u64;
    let mut redirect_max_wait_cycles = 0u32;
    let mut redirect_wait_histogram = [0u32; 32];
    let mut opcode_retired = [0u32; 16];
    let mut sdram_state_cycles = [0u64; 11];

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

    for cycle in 0..maximum_cycles {
        let reset = cycle < 2;
        set_bit(core_input.reset, reset, &mut circuit);
        set_bit(fetch_input.reset, reset, &mut circuit);
        set_bit(icache_input.reset, reset, &mut circuit);
        set_bit(dcache_input.reset, reset, &mut circuit);
        set_bit(arbiter_input.reset, reset, &mut circuit);

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

        if !reset {
            let retired = core.retired_words as u32;
            if retired != previous_retired {
                if let Some((origin, instruction)) = last_fetched {
                    opcode_retired[usize::from(instruction >> 12)] =
                        opcode_retired[usize::from(instruction >> 12)].wrapping_add(1);
                    let target = physical_pc(core.code_segment as u16, core.pc as u16);
                    if target != next_physical_word(origin) {
                        redirect_count = redirect_count.wrapping_add(1);
                        pending_redirect = Some((origin, target, instruction, cycle));
                    }
                }
                previous_retired = retired;
            }

            if core.instruction_request_valid {
                if fetch.core_request_ready {
                    instruction_fetches = instruction_fetches.wrapping_add(1);
                    let address = physical_pc(core.code_segment as u16, core.pc as u16);
                    last_fetched = Some((address, fetch.core_read_data as u16));
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
            } else if core.data_request_valid {
                data_request_cycles += 1;
                if dcache.cpu_request_ready {
                    data_requests = data_requests.wrapping_add(1);
                    pending_data_op = Some(PendingDataOp {
                        is_write: core.data_write,
                        accept_cycle: cycle,
                    });
                }
            } else if core.data_response_ready {
                data_response_cycles += 1;
            } else if !core.halted && !core.fault {
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
            if dcache.memory_request_valid {
                if arb.memory_line {
                    dcache_line_requests = dcache_line_requests.wrapping_add(1);
                    if arb.memory_write {
                        dcache_writebacks = dcache_writebacks.wrapping_add(1);
                    } else {
                        dcache_refills = dcache_refills.wrapping_add(1);
                        if pending_data_op.map_or(false, |op| op.is_write) {
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
        }
        if sdram.state == SdramState::RefreshWait {
            refreshes = refreshes.wrapping_add(1);
        }

        circuit.clock_tick();

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

        if core.halted {
            let result = BenchResult {
                program_words: words.len(),
                cycles: cycle + 1,
                halt_signal: core.halt_signal as u16,
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
                data_requests,
                icache_line_requests,
                dcache_line_requests,
                dcache_refills,
                dcache_load_refills,
                dcache_store_refills,
                dcache_writebacks,
                dcache_word_requests,
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
    panic!("benchmark exceeded {maximum_cycles} cycles");
}

#[cfg(test)]
mod tests {
    use super::*;
    use cpu_v3::rcc_backend::{self, CompilerOptions};
    use rcc::frontend::compile_program_named;
    use std::env;
    use std::fs::{read_dir, read_to_string};
    use std::path::PathBuf;

    const CONTROL_FLOW_SOURCE: &str = r#"
fn leaf(x: u16) -> u16 {
    x + 1
}

fn main() {
    let mut i: u16 = 0;
    let mut sum: u16 = 0;
    while i < 256 {
        sum = leaf(sum);
        i = i + 1;
    }
    if sum == 256 {
        halt(1);
    } else {
        halt(0);
    }
}
"#;

    const DATA_SOURCE: &str = r#"
use crate::dsl_rt::*;

const N: u16 = 128;
static DATA: [u16; 128] = [0; 128];

fn main() {
    let mut d = DATA.as_array();
    let mut i: u16 = 0;
    while i < N {
        d[i] = i ^ 0x5a5a;
        i = i + 1;
    }
    let mut sum: u16 = 0;
    i = 0;
    while i < N {
        sum = sum + d[i];
        i = i + 1;
    }
    if sum != 0 {
        halt(1);
    } else {
        halt(0);
    }
}
"#;

    fn compile(source: &str) -> Vec<u16> {
        let options = CompilerOptions::default();
        let program = compile_program_named("bench", source, &options, &mut |_| {
            Err("benchmark uses no modules".to_string())
        })
        .unwrap();
        rcc_backend::compile(program, &options, "main").words
    }

    fn trace_directory(name: &str) -> PathBuf {
        env::var_os("CPU_V3_BENCH_OUTPUT")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../..")
                    .join("target/cpu-v3-bench")
            })
            .join(name)
    }

    #[test]
    fn control_flow_probe_records_calls_returns_and_taken_loop_edges() {
        let words = compile(CONTROL_FLOW_SOURCE);
        let trace_directory = trace_directory("control-flow");
        let result = run_benchmark_profiled(&words, 1_000_000, Some(&trace_directory));
        assert_eq!(result.halt_signal, 1);
        assert!(result.redirect_count >= 3 * 256);
        assert!(result.redirect_wait_histogram[2] >= 3 * 256);
    }

    #[test]
    fn data_probe_separates_write_through_stores_from_line_refills() {
        let words = compile(DATA_SOURCE);
        let trace_directory = trace_directory("data");
        let result = run_benchmark_profiled(&words, 1_000_000, Some(&trace_directory));
        assert_eq!(result.halt_signal, 1);
        assert!(result.dcache_word_requests >= 128);
        assert!(result.dcache_line_requests >= 8);
        assert_eq!(result.data_requests, result.dcache_word_requests + 128);
    }

    #[test]
    fn smoke_halt_runs_to_completion() {
        let words = compile("fn main() { halt(7); }");
        let result = run_benchmark(&words, 100_000);
        println!(
            "smoke halt={} cycles={} retired={}",
            result.halt_signal, result.cycles, result.retired_words
        );
        assert_eq!(result.halt_signal, 7);
    }

    #[allow(dead_code)]
    mod benchmark_suite {
        use super::*;

        const QUICKSORT_SOURCE: &str = include_str!("../benchmarks/algorithms/quicksort.rs");

        const INT_SHORT_ALU_SOURCE: &str = r#"
fn main() {
    let mut x: u16 = 0x1357;
    let mut y: u16 = 0x2468;
    let mut i: u16 = 0;
    while i < 24 {
        x = (x + y) ^ (x << 1);
        y = (y + 3) ^ (x >> 2);
        i = i + 1;
    }
    halt(1);
}
"#;

        const INT_SHORT_BRANCH_SOURCE: &str = r#"
fn main() {
    let mut x: u16 = 0;
    let mut i: u16 = 0;
    while i < 48 {
        if (i & 3) == 0 { x = x + 7; }
        else if (i & 1) == 0 { x = x ^ i; }
        else { x = x - 1; }
        i = i + 1;
    }
    halt(1);
}
"#;

        const INT_SHORT_MEMORY_SOURCE: &str = r#"
use crate::dsl_rt::*;
static DATA: [u16; 48] = [0; 48];
fn main() {
    let mut d = DATA.as_array();
    let mut i: u16 = 0;
    let mut sum: u16 = 0;
    while i < 48 { d[i] = ((i << 3) + i) ^ 0x55aa; i = i + 1; }
    i = 0;
    while i < 48 { sum = sum + d[i]; i = i + 1; }
    if sum != 0 { halt(1); } else { halt(0); }
}
"#;

        const INT_SHORT_MIXED_SOURCE: &str = r#"
fn mix(x0: u16, n: u16) -> u16 {
    let mut x: u16 = x0;
    let mut i: u16 = 0;
    while i < n {
        x = ((x << 3) ^ (x >> 2)) + i + 0x1234;
        if (x & 7) == 3 { x = x ^ 0xa5a5; }
        i = i + 1;
    }
    x
}
fn main() { let x = mix(7, 24); if x != 0 { halt(1); } else { halt(0); } }
"#;

        const INT_MEDIUM_ALU_SOURCE: &str = r#"
fn main() {
    let mut x: u16 = 1;
    let mut y: u16 = 0x9e37;
    let mut i: u16 = 0;
    while i < 1536 {
        x = (x + y) ^ (x << 5) ^ (x >> 3);
        y = y + x + i;
        i = i + 1;
    }
    halt(1);
}
"#;

        const INT_MEDIUM_MEMORY_SOURCE: &str = r#"
use crate::dsl_rt::*;
static DATA: [u16; 1024] = [0; 1024];
fn main() {
    let mut d = DATA.as_array();
    let mut i: u16 = 0;
    let mut sum: u16 = 0;
    while i < 1024 { d[i] = (i << 3) ^ (i >> 2) ^ 0x6d2b; i = i + 1; }
    i = 0;
    while i < 1024 { sum = sum + d[i]; i = i + 1; }
    if sum != 0 { halt(1); } else { halt(0); }
}
"#;

        const STREAMING_MIX_SOURCE: &str = r#"
use crate::dsl_rt::*;
const N: u16 = 4096;
static INPUT_A: [u16; 4096] = [0; 4096];
static INPUT_B: [u16; 4096] = [0; 4096];
static OUTPUT: [u16; 4096] = [0; 4096];
fn main() {
    let mut a = INPUT_A.as_array();
    let mut b = INPUT_B.as_array();
    let mut out = OUTPUT.as_array();
    let mut i: u16 = 0;
    while i < N { a[i] = i ^ 0x5a5a; b[i] = (i << 1) + 3; i = i + 1; }
    i = 0;
    while i < N {
        out[i] = (a[i] + b[i]) ^ (a[i] >> 3) ^ (b[i] << 2);
        i = i + 1;
    }
    i = 0;
    while i < N {
        if out[i] != ((a[i] + b[i]) ^ (a[i] >> 3) ^ (b[i] << 2)) { halt(0); }
        i = i + 1;
    }
    halt(1);
}
"#;

        const STREAMING_BALANCED_SOURCE: &str = r#"
use crate::dsl_rt::*;
const N: u16 = 4096;
const B_OFFSET: u16 = 4112;
const OUT_OFFSET: u16 = 8224;
static DATA: [u16; 12320] = [0; 12320];
fn main() {
    let mut d = DATA.as_array();
    let mut i: u16 = 0;
    while i < N {
        d[i] = i ^ 0x5a5a;
        d[B_OFFSET + i] = (i << 1) + 3;
        i = i + 1;
    }
    i = 0;
    while i < N {
        d[OUT_OFFSET + i] =
            (d[i] + d[B_OFFSET + i]) ^ (d[i] >> 3) ^ (d[B_OFFSET + i] << 2);
        i = i + 1;
    }
    i = 0;
    while i < N {
        let expected =
            (d[i] + d[B_OFFSET + i]) ^ (d[i] >> 3) ^ (d[B_OFFSET + i] << 2);
        if d[OUT_OFFSET + i] != expected { halt(0); }
        i = i + 1;
    }
    halt(1);
}
"#;

        fn generated_int_icache_jump() -> Vec<u16> {
            let mut words = Vec::new();
            words.extend(load_immediate16(1, 0x1357));
            words.extend(load_immediate16(2, 0x2468));
            for _ in 0..144 {
                for lane in 0..10 {
                    words.push(alu(AluOp::Add, 1, 1, 2));
                    words.push(alu(AluOp::Xor, 2, 2, 1));
                    words.push(alu(AluOp::ShiftLeft, 1, 1, 2 + (lane & 1)));
                }
                words.push(jump_relative(1));
                words.push(nop());
            }
            words.extend(load_immediate16(0, 1));
            words.push(halt());
            words
        }

        fn generated_fpu_short(op: FpuOp, left: u16, right: u16) -> Vec<u16> {
            let mut words = Vec::new();
            words.extend(load_immediate16(1, left));
            words.extend(load_immediate16(2, right));
            words.push(fpu(FpuOp::Load, 0, 1));
            words.push(fpu(FpuOp::Load, 1, 2));
            words.push(fpu(op, 0, 1));
            words.push(fpu(FpuOp::Store, 0, 0));
            words.push(halt());
            words
        }

        fn generated_fpu_unary() -> Vec<u16> {
            let mut words = Vec::new();
            words.extend(load_immediate16(1, 0xff00));
            words.push(fpu(FpuOp::Load, 0, 1));
            words.push(fpu_unary(0, FpuUnaryOp::Abs));
            words.push(fpu(FpuOp::Store, 0, 0));
            words.push(halt());
            words
        }

        fn generated_fpu_long() -> Vec<u16> {
            let mut words = Vec::new();
            words.extend(load_immediate16(1, 256));
            words.extend(load_immediate16(2, 1));
            words.push(fpu(FpuOp::Load, 0, 1));
            words.push(fpu(FpuOp::Load, 1, 2));
            for index in 0..3072 {
                words.push(fpu(
                    if index & 1 == 0 {
                        FpuOp::Add
                    } else {
                        FpuOp::Sub
                    },
                    0,
                    1,
                ));
            }
            words.push(fpu(FpuOp::Store, 0, 0));
            words.push(halt());
            words
        }

        fn run_case(name: &str, source: &str, maximum_cycles: usize, prefetch_enabled: bool) {
            let words = compile(source);
            run_words_case(name, &words, maximum_cycles, prefetch_enabled, 1);
        }

        fn run_words_case(
            name: &str,
            words: &[u16],
            maximum_cycles: usize,
            prefetch_enabled: bool,
            expected_halt: u16,
        ) {
            let trace_name = if prefetch_enabled {
                name.to_string()
            } else {
                format!("stage2-{name}")
            };
            let trace_directory = trace_directory(&trace_name);
            let result = run_benchmark_profiled_with_prefetch(
                words,
                maximum_cycles,
                Some(&trace_directory),
                prefetch_enabled,
            );
            assert_eq!(
                result.halt_signal, expected_halt,
                "{name} self-check failed"
            );
            let loads = result.opcode_retired[8];
            let stores = result.opcode_retired[9];
            let retired_words = result.retired_words.max(1) as f64;
            let fetch_wait_percent = 100.0 * result.fetch_wait_cycles as f64 / result.cycles as f64;
            let data_path_percent = 100.0
                * (result.data_request_cycles + result.data_response_cycles) as f64
                / result.cycles as f64;
            let avg_load = if loads == 0 {
                0.0
            } else {
                result.load_latency_cycles as f64 / f64::from(loads)
            };
            let avg_store = if stores == 0 {
                0.0
            } else {
                result.store_latency_cycles as f64 / f64::from(stores)
            };
            println!(
            "BENCH name={name} program_words={} cycles={} retired_words={} cpi={:.6} cpw={:.6} fetch_wait_pct={fetch_wait_percent:.3} data_path_pct={data_path_percent:.3} data_req_cycles={} data_resp_cycles={} data_requests={} loads={loads} stores={stores} avg_load_latency={avg_load:.3} avg_store_latency={avg_store:.3} dcache_word_requests={} dcache_line_requests={} icache_line_requests={} redirects={} redirect_wait={} prefetch_issued={} prefetch_useful={} prefetch_useless={} prefetch_dropped={} refreshes={}",
            result.program_words,
            result.cycles,
            result.retired_words,
            result.cycles as f64 / retired_words,
            result.cycles as f64 / retired_words,
            result.data_request_cycles,
            result.data_response_cycles,
            result.data_requests,
            result.dcache_word_requests,
            result.dcache_line_requests,
            result.icache_line_requests,
            result.redirect_count,
            result.redirect_wait_cycles,
            result.prefetch_issued,
            result.prefetch_useful,
            result.prefetch_useless,
            result.prefetch_dropped,
            result.refreshes,
        );
        }

        fn metadata_value(source: &str, key: &str) -> Option<String> {
            source.lines().find_map(|line| {
                let line = line
                    .trim()
                    .strip_prefix("//")
                    .or_else(|| line.trim().strip_prefix('#'))?
                    .trim();
                let (candidate, value) = line.split_once(':')?;
                (candidate.trim() == key).then(|| value.trim().to_string())
            })
        }

        fn parse_hex(source: &str, path: &Path) -> Vec<u16> {
            source
                .lines()
                .filter_map(|line| {
                    let word = line.split('#').next().unwrap_or_default().trim();
                    (!word.is_empty()).then_some(word)
                })
                .map(|word| {
                    u16::from_str_radix(word.trim_start_matches("0x"), 16).unwrap_or_else(|error| {
                        panic!("invalid word {word:?} in {}: {error}", path.display())
                    })
                })
                .collect()
        }

        #[test]
        #[ignore = "explicit release-mode folder-driven benchmark suite"]
        fn run_benchmark_directory() {
            let input_root = env::var_os("CPU_V3_BENCH_DIR")
                .map(PathBuf::from)
                .expect("set CPU_V3_BENCH_DIR to a benchmark program directory");
            let mut paths = read_dir(&input_root)
                .unwrap_or_else(|error| panic!("cannot read {}: {error}", input_root.display()))
                .map(|entry| entry.expect("cannot read benchmark directory entry").path())
                .filter(|path| {
                    matches!(
                        path.extension().and_then(|value| value.to_str()),
                        Some("rs" | "hex")
                    )
                })
                .collect::<Vec<_>>();
            paths.sort();
            assert!(
                !paths.is_empty(),
                "{} contains no .rs or .hex benchmarks",
                input_root.display()
            );

            for path in paths {
                let source = read_to_string(&path)
                    .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
                let maximum_cycles = metadata_value(&source, "bench-max-cycles")
                    .unwrap_or_else(|| panic!("{} lacks bench-max-cycles metadata", path.display()))
                    .parse::<usize>()
                    .unwrap_or_else(|error| {
                        panic!("invalid bench-max-cycles in {}: {error}", path.display())
                    });
                let expected_halt = metadata_value(&source, "bench-expected-halt")
                    .unwrap_or_else(|| "1".to_string())
                    .parse::<u16>()
                    .unwrap_or_else(|error| {
                        panic!("invalid bench-expected-halt in {}: {error}", path.display())
                    });
                let name = path
                    .file_stem()
                    .and_then(|value| value.to_str())
                    .expect("benchmark filename must be UTF-8");
                let words = match path.extension().and_then(|value| value.to_str()) {
                    Some("rs") => compile(&source),
                    Some("hex") => parse_hex(&source, &path),
                    _ => unreachable!(),
                };
                run_words_case(name, &words, maximum_cycles, true, expected_halt);
            }
        }

        fn run_suite(prefetch_enabled: bool) {
            run_case(
                "int-short-alu",
                INT_SHORT_ALU_SOURCE,
                10_000,
                prefetch_enabled,
            );
            run_case(
                "int-short-branch",
                INT_SHORT_BRANCH_SOURCE,
                10_000,
                prefetch_enabled,
            );
            run_case(
                "int-short-memory",
                INT_SHORT_MEMORY_SOURCE,
                10_000,
                prefetch_enabled,
            );
            run_case(
                "int-short-mixed",
                INT_SHORT_MIXED_SOURCE,
                10_000,
                prefetch_enabled,
            );
            run_case(
                "int-medium-alu",
                INT_MEDIUM_ALU_SOURCE,
                400_000,
                prefetch_enabled,
            );
            run_case(
                "int-medium-memory",
                INT_MEDIUM_MEMORY_SOURCE,
                600_000,
                prefetch_enabled,
            );
            run_case(
                "quicksort-4096",
                QUICKSORT_SOURCE,
                30_000_000,
                prefetch_enabled,
            );
            run_words_case(
                "int-icache-jump",
                &generated_int_icache_jump(),
                1_000_000,
                prefetch_enabled,
                1,
            );
            run_words_case(
                "fpu-short-add",
                &generated_fpu_short(FpuOp::Add, 256, 512),
                10_000,
                prefetch_enabled,
                768,
            );
            run_words_case(
                "fpu-short-mul",
                &generated_fpu_short(FpuOp::Mul, 384, 512),
                10_000,
                prefetch_enabled,
                768,
            );
            run_words_case(
                "fpu-short-unary",
                &generated_fpu_unary(),
                10_000,
                prefetch_enabled,
                256,
            );
            run_words_case(
                "fpu-long-mixed",
                &generated_fpu_long(),
                1_000_000,
                prefetch_enabled,
                256,
            );
            run_case(
                "streaming-mix",
                STREAMING_MIX_SOURCE,
                8_000_000,
                prefetch_enabled,
            );
            run_case(
                "streaming-balanced",
                STREAMING_BALANCED_SOURCE,
                8_000_000,
                prefetch_enabled,
            );
        }
    }
}
