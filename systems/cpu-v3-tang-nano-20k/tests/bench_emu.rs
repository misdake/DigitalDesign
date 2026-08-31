//! Cycle-accurate full-system emulator for CpuV3 performance benchmarks.
//!
//! Wires the core, instruction fetch queue, I-cache, D-cache and the memory
//! arbiter through their Rust emulators, and drives them against a cycle-faithful
//! model of the Tang Nano 20K SDRAM word port (ACTIVE / READ 4x64 / WRITE /
//! RECOVERY / periodic refresh). It reports the total cycle count and the
//! I-cache prefetch counters, so a workload can be profiled without Icarus or
//! hardware.

use cpu_v3::{
    CpuV3Core, CpuV3CoreInput, CpuV3CoreOutput, CpuV3DataCache, CpuV3DataCacheInput,
    CpuV3DataCacheOutput, CpuV3InstructionFetchQueue, CpuV3InstructionFetchQueueInput,
    CpuV3InstructionFetchQueueOutput, CpuV3TwoWayCache, CpuV3TwoWayCacheInput,
    CpuV3TwoWayCacheOutput,
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
    pub dcache_word_requests: u32,
    pub refreshes: u32,
    pub redirect_count: u32,
    pub redirect_wait_cycles: u64,
    pub redirect_max_wait_cycles: u32,
    pub redirect_wait_histogram: [u32; 32],
    pub opcode_retired: [u32; 16],
    pub sdram_state_cycles: [u64; 11],
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
        writeln!(
            writer,
            "dcache_load_hit_percent={:.6}",
            if loads == 0 {
                0.0
            } else {
                100.0 * f64::from(loads.saturating_sub(result.dcache_line_requests))
                    / f64::from(loads)
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

        // fetch queue -> I-cache
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
        // order matches the combinational dependency (core, fetch, caches,
        // arbiter).
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
    let mut instruction_fetches = 0u32;
    let mut data_requests = 0u32;
    let mut icache_line_requests = 0u32;
    let mut dcache_line_requests = 0u32;
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
                }
            } else if core.data_response_ready {
                data_response_cycles += 1;
            } else if !core.halted && !core.fault {
                execute_cycles += 1;
            }
        }

        if arb.memory_request_valid && sdram.request_ready() {
            if dcache.memory_request_valid {
                if arb.memory_line {
                    dcache_line_requests = dcache_line_requests.wrapping_add(1);
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
                dcache_word_requests,
                refreshes,
                redirect_count,
                redirect_wait_cycles,
                redirect_max_wait_cycles,
                redirect_wait_histogram,
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
    use std::path::PathBuf;

    const QSORT_SOURCE: &str = r#"
use crate::dsl_rt::*;

const N: u16 = 2048;

static DATA: [u16; 2048] = [0; 2048];

fn qsort(mut d: Array<u16>, lo: u16, hi: u16) {
    if lo < hi {
        let mid = (lo + hi) >> 1;
        let tmp = d[mid];
        d[mid] = d[hi];
        d[hi] = tmp;

        let pivot = d[hi];
        let mut i: u16 = lo;
        let mut j: u16 = lo;
        while j < hi {
            if d[j] < pivot {
                let swap = d[i];
                d[i] = d[j];
                d[j] = swap;
                i = i + 1;
            }
            j = j + 1;
        }
        let swap = d[i];
        d[i] = d[hi];
        d[hi] = swap;

        if lo < i {
            qsort(d, lo, i - 1);
        }
        if i < hi {
            qsort(d, i + 1, hi);
        }
    }
}

fn verify(d: Array<u16>) -> u16 {
    let mut i: u16 = 1;
    while i < N {
        if d[i - 1] > d[i] {
            return 0;
        }
        i = i + 1;
    }
    1
}

fn checksum(d: Array<u16>) -> u16 {
    let mut sum: u16 = 0;
    let mut i: u16 = 0;
    while i < N {
        sum = sum + d[i];
        i = i + 1;
    }
    sum
}

#[allow(clippy::eq_op)]
fn main() {
    let mut d = DATA.as_array();
    let mut i: u16 = 0;
    while i < N {
        d[i] = (i << 5) ^ (i >> 6) ^ (i << 11) ^ 0x9e37;
        i = i + 1;
    }
    let before = checksum(d);
    qsort(d, 0, N - 1);
    let ok = verify(d);
    let after = checksum(d);
    if before == after {
        halt(ok);
    } else {
        halt(0);
    }
}
"#;

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
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join("target/cpu-v3-bench")
            .join(name)
    }

    #[test]
    #[ignore = "explicit release-mode cycle-accurate full-system benchmark"]
    fn quicksort_runs_on_the_cycle_accurate_emu() {
        let words = compile(QSORT_SOURCE);
        let trace_directory = trace_directory("quicksort");
        let result = run_benchmark_profiled(&words, 50_000_000, Some(&trace_directory));
        println!(
            "halt={} cycles={} retired={} fetch_wait={} execute={} data_req={} data_resp={} redirects={} redirect_wait={} prefetch issued={} useful={} useless={} dropped={}",
            result.halt_signal,
            result.cycles,
            result.retired_words,
            result.fetch_wait_cycles,
            result.execute_cycles,
            result.data_request_cycles,
            result.data_response_cycles,
            result.redirect_count,
            result.redirect_wait_cycles,
            result.prefetch_issued,
            result.prefetch_useful,
            result.prefetch_useless,
            result.prefetch_dropped,
        );
        assert_eq!(
            result.halt_signal, 1,
            "quicksort must verify its sorted order"
        );
        assert!(result.prefetch_issued > 0, "I-cache prefetch must issue");
        assert!(
            result.prefetch_useful > 0,
            "I-cache prefetch must be consumed"
        );
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
    fn data_probe_write_allocates_and_keeps_store_hits_off_sdram() {
        let words = compile(DATA_SOURCE);
        let trace_directory = trace_directory("data");
        let result = run_benchmark_profiled(&words, 1_000_000, Some(&trace_directory));
        assert_eq!(result.halt_signal, 1);
        assert_eq!(result.dcache_word_requests, 0);
        assert!(result.dcache_line_requests >= 8);
        assert_eq!(result.data_requests, 256);
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
}
