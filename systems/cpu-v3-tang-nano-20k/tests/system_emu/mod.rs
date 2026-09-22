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
use cpu_v3_tang_nano_20k::{
    CpuV3MemoryArbiter, CpuV3MemoryArbiterInput, CpuV3MemoryArbiterOutput, CpuV3MemoryArbiterState,
    SYSTEM_CONTROL_DEVICE,
};
use digital_design_circuit::{build_circuit, Circuit, Wire, Wires};
use digital_design_hardware::{Module, ModuleIo};
use rcc::frontend::compile_program_named;
use std::collections::VecDeque;
use std::fs::{create_dir_all, File};
use std::io::{BufWriter, Write};
use std::path::Path;

/// System-control channel carrying the fetch-pause register in the modelled
/// device: a non-zero write holds the frontend off the core, a zero write
/// releases it. The emulator owns this hold; there is no hardware port.
pub const FETCH_PAUSE_CHANNEL: u8 = 6;

// ---- complete-machine snapshot / restore (design/fetch-pause section 7.1) ----
//
// `Circuit` cannot be cloned (it owns `Box<dyn External>`), so a snapshot is
// "clone every emulated module state + the SDRAM image" and a restore writes
// those states back into the live circuit. The handles below are the same
// `Rc<RefCell<..>>` pattern the fetch queue already used, attached for every
// emulated module instead of only the queue.

type CoreHandle = std::rc::Rc<std::cell::RefCell<cpu_v3::CpuV3CoreState>>;
type IcacheHandle = std::rc::Rc<std::cell::RefCell<cpu_v3::CpuV3TwoWayCacheState>>;
type DcacheHandle = std::rc::Rc<std::cell::RefCell<cpu_v3::CpuV3DataCacheState>>;
type ArbiterHandle = std::rc::Rc<std::cell::RefCell<CpuV3MemoryArbiterState>>;

/// One emulated module: its state handle plus the wires it is bound to, so it
/// can be driven from the harness exactly like `emu_connect` would.
struct ObservedModule<M: digital_design_hardware::Module> {
    state: std::rc::Rc<std::cell::RefCell<M::EmuState>>,
    input: M::Input,
    output: M::Output,
}

impl<M: digital_design_hardware::Module> ObservedModule<M> {
    fn new(
        state: std::rc::Rc<std::cell::RefCell<M::EmuState>>,
        input: M::Input,
        output: M::Output,
    ) -> Self {
        Self {
            state,
            input,
            output,
        }
    }
}

impl<M: digital_design_hardware::Module> digital_design_circuit::External for ObservedModule<M> {
    fn execute(&mut self, circuit: &mut digital_design_circuit::CircuitWires) {
        M::execute_emu(
            &mut self.state.borrow_mut(),
            circuit,
            &self.input,
            &self.output,
        );
    }
    fn clock(&mut self, circuit: &mut digital_design_circuit::CircuitWires) {
        M::clock_emu(
            &mut self.state.borrow_mut(),
            circuit,
            &self.input,
            &self.output,
        );
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// A complete machine snapshot: every emulated module state plus architectural
/// memory. Cloning it costs one copy of the caches and memory, which is what
/// makes checkpointing cheap relative to re-running a prefix.
#[derive(Clone)]
pub struct SystemSnapshot {
    core: cpu_v3::CpuV3CoreState,
    fetch: cpu_v3::CpuV3InstructionFetchQueueState,
    icache: cpu_v3::CpuV3TwoWayCacheState,
    dcache: cpu_v3::CpuV3DataCacheState,
    arbiter: CpuV3MemoryArbiterState,
    /// SDRAM contents, including anything not yet written back from the D-cache.
    pub memory: Vec<u16>,
    /// The SDRAM port model itself, so a snapshot taken while a transaction is in
    /// flight also restores the controller phase (`state`, `beat`, `read_delay`,
    /// `pending_*`, `recovery_count`, refresh counter, response beat).
    sdram: SdramModel,
    /// Pause request state at the snapshot point.
    paused: bool,
}

/// Read-only architectural state of the cycle model, for direct comparison
/// against the naive `CpuV3Sim`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArchitecturalView {
    pub gprs: [u16; 16],
    pub f_registers: [i32; 64],
    pub accumulator: i64,
    pub pc: u16,
    pub segments: (u16, u16),
    pub retired_words: u32,
}

/// Live handles to the emulated machine, used to take and apply snapshots.
pub struct SystemHandles {
    core: CoreHandle,
    fetch: std::rc::Rc<std::cell::RefCell<cpu_v3::CpuV3InstructionFetchQueueState>>,
    icache: IcacheHandle,
    dcache: DcacheHandle,
    arbiter: ArbiterHandle,
}

impl SystemHandles {
    /// Read-only architectural view of the core at this instant: 16 GPRs, 64 F
    /// registers, the accumulator, the PC and the segment registers. The system
    /// harness compares these directly against the naive simulator.
    pub fn architectural_view(&self) -> ArchitecturalView {
        let core = self.core.borrow();
        ArchitecturalView {
            gprs: core.architectural_gprs(),
            f_registers: core.architectural_f_registers(),
            accumulator: core.accumulator(),
            pc: core.program_counter(),
            segments: core.segments(),
            retired_words: core.retired_words(),
        }
    }

    /// Captures the whole machine. `memory` must be the SDRAM image at the same
    /// instant, and `sdram` the port model at that instant.
    pub fn snapshot(&self, memory: Vec<u16>, sdram: SdramModel, paused: bool) -> SystemSnapshot {
        SystemSnapshot {
            core: self.core.borrow().clone(),
            fetch: self.fetch.borrow().clone(),
            icache: self.icache.borrow().clone(),
            dcache: self.dcache.borrow().clone(),
            arbiter: self.arbiter.borrow().clone(),
            memory,
            sdram,
            paused,
        }
    }

    /// Writes a snapshot back into the live machine, so execution continues from
    /// exactly the snapshotted point. The SDRAM model lives outside the circuit
    /// and is restored by the caller through [`SystemSnapshot::sdram`].
    pub fn restore(&self, snapshot: &SystemSnapshot) {
        *self.core.borrow_mut() = snapshot.core.clone();
        *self.fetch.borrow_mut() = snapshot.fetch.clone();
        *self.icache.borrow_mut() = snapshot.icache.clone();
        *self.dcache.borrow_mut() = snapshot.dcache.clone();
        *self.arbiter.borrow_mut() = snapshot.arbiter.clone();
    }
}

// Observe the actual fetch emulator state without adding synthesized ports or
// running a second model. Other system/co-sim paths retain normal emu_connect.
//
// The system-control device's `fetch_pause` is an emulator-side hold. The queue
// keeps being clocked in lockstep with the core - its pop and the core's latch
// happen on the same edge, so stopping its clock would desynchronise the two -
// and it keeps seeing the core's request, so the word the core is waiting for
// stays in the queue and can be re-offered on release. Only the answer the core
// samples is suppressed, and that is done at the core's own input in the run
// loop (`instruction_response_valid` / `instruction_request_ready` are forced
// low while paused), which is the signal the core actually reads.
struct ObservedFetch {
    state: std::rc::Rc<std::cell::RefCell<cpu_v3::CpuV3InstructionFetchQueueState>>,
    input: CpuV3InstructionFetchQueueInput,
    output: CpuV3InstructionFetchQueueOutput,
}
impl digital_design_circuit::External for ObservedFetch {
    fn execute(&mut self, circuit: &mut digital_design_circuit::CircuitWires) {
        CpuV3InstructionFetchQueue::execute_emu(
            &mut self.state.borrow_mut(),
            circuit,
            &self.input,
            &self.output,
        );
    }
    fn clock(&mut self, circuit: &mut digital_design_circuit::CircuitWires) {
        CpuV3InstructionFetchQueue::clock_emu(
            &mut self.state.borrow_mut(),
            circuit,
            &self.input,
            &self.output,
        );
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

fn write_btc_diagnostics(directory: Option<&Path>, stats: cpu_v3::CpuV3BtcStatistics) {
    let Some(directory) = directory else {
        return;
    };
    let mut out = BufWriter::new(File::create(directory.join("btc.txt")).unwrap());
    writeln!(out, "btc_entries={}", cpu_v3::CPU_V3_BTC_ENTRIES).unwrap();
    writeln!(out, "lookups={}", stats.lookups).unwrap();
    writeln!(out, "complete_hits={}", stats.hits).unwrap();
    writeln!(out, "installed={}", stats.installed).unwrap();
    writeln!(out, "cancelled_fills={}", stats.cancelled_fills).unwrap();
    writeln!(out, "accepted_words={}", stats.accepted_words).unwrap();
    writeln!(out, "aborted_replays={}", stats.aborted_replays).unwrap();
    writeln!(
        out,
        "continuation_wait_cycles={}",
        stats.continuation_wait_cycles
    )
    .unwrap();
}

const BENCHMARK_SDRAM_WORDS: usize = 0x10000;
const SYSTEM_COSIM_SDRAM_WORDS: usize = 0x20000;

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

/// Cycle-faithful model of `SharedSdramPort` for the CPU port only. Refresh is
/// due every 600 clocks; a line read costs ACTIVE + READ + four 64-bit beats + three
/// recovery clocks.
///
/// Cloneable because every field is part of the machine state a snapshot has to
/// carry: the `memory` contents alone are not enough, since a snapshot taken
/// while a transaction is in flight must also restore the controller phase.
#[derive(Clone)]
pub struct SdramModel {
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
        // like the `SharedSdramPort` RTL (refresh_due = refresh_count >= 600). The
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
                    } else if write {
                        // Word write: the full four-beat ST_WRITE_STAGE keeps
                        // the gearbox write_buffer capture pointer aligned,
                        // mirroring the RTL port.
                        self.beat = 0;
                        self.state = SdramState::WriteStage;
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
    /// Cycles during which the modelled system-control fetch pause was held.
    pub fetch_pause_cycles: u32,
    /// Retired word count at the first paused cycle, when a pause was requested.
    pub retired_words_at_first_pause: Option<u32>,
    /// Cycles on which the data side was busy: the core had a data request in
    /// flight or the D-cache was not accepting requests (refill, eviction,
    /// write-back or maintenance). Writes retired just before a pause keep
    /// draining for a while after it, so a comparison is only valid once this
    /// has been false for a cycle while the pause is held.
    pub data_side_busy_cycles: u32,
    /// First cycle at or after the pause engaged on which the data side was
    /// quiet. `None` when no pause was requested.
    pub data_side_quiet_at: Option<usize>,
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
/// the core halts, returning the cycle count and the profile counters.
pub fn run_benchmark(words: &[u16], maximum_cycles: usize) -> BenchResult {
    run_benchmark_paused_memory(words, maximum_cycles, |_, _| false).0
}

/// Runs `words` with a per-cycle hook on the modelled system-control fetch
/// pause: returning `true` holds the fetch queue off the core for that cycle.
/// The pause is emulator-owned (device 0 channel 6); there is no hardware port.
pub fn run_benchmark_paused(
    words: &[u16],
    maximum_cycles: usize,
    pause_at: impl FnMut(usize, u32) -> bool,
) -> BenchResult {
    run_benchmark_paused_memory(words, maximum_cycles, pause_at).0
}

/// As `run_benchmark_paused`, but also returns the final SDRAM image after the
/// post-halt D-cache clean, for comparing architectural memory against another
/// model.
pub fn run_benchmark_paused_memory(
    words: &[u16],
    maximum_cycles: usize,
    mut pause_at: impl FnMut(usize, u32) -> bool,
) -> (BenchResult, Vec<u16>) {
    let (result, memory, _) =
        run_benchmark_profiled_inner(words, maximum_cycles, None, &mut pause_at, None, None);
    (result, memory)
}

/// A checkpoint taken from inside a run: the complete machine snapshot.
pub struct Checkpoint {
    pub snapshot: SystemSnapshot,
}

impl Checkpoint {
    /// True when the SDRAM port model was not idle at the snapshot point, i.e.
    /// a transaction phase had to be carried in the snapshot for the resume to
    /// be exact.
    pub fn sdram_was_busy(&self) -> bool {
        self.snapshot.sdram.state != SdramState::Idle
    }
}

/// Per-cycle checkpoint observer: `(cycle, handles, sdram_busy) -> take now?`.
/// It is offered the live machine every cycle once the pause is held and the
/// data side has gone quiet (so the architectural state is stable), and it may
/// take more than one checkpoint by returning `true` more than once.
type CheckpointFn<'a> = &'a mut dyn FnMut(usize, &SystemHandles, bool) -> bool;

/// As `run_benchmark_paused_memory`, but able to start from a checkpoint and to
/// hand one back.
///
/// * `start_from` restores a previously taken checkpoint before the run begins,
///   so a trace's prefix is reused instead of re-run.
/// * `checkpoint_at` is offered the live machine at the end of every cycle while
///   it is paused and the data side is quiescent (so the architectural state is
///   stable). Return `true` to take a checkpoint; the first one accepted is
///   returned.
pub fn run_from_checkpoint(
    words: &[u16],
    maximum_cycles: usize,
    mut pause_at: impl FnMut(usize, u32) -> bool,
    start_from: Option<&Checkpoint>,
    checkpoint_at: Option<CheckpointFn<'_>>,
) -> (BenchResult, Vec<u16>, Option<Checkpoint>) {
    run_benchmark_profiled_inner(
        words,
        maximum_cycles,
        None,
        &mut pause_at,
        start_from,
        checkpoint_at,
    )
}

pub fn run_benchmark_profiled(
    words: &[u16],
    maximum_cycles: usize,
    trace_directory: Option<&Path>,
) -> BenchResult {
    run_benchmark_profiled_inner(
        words,
        maximum_cycles,
        trace_directory,
        &mut |_, _| false,
        None,
        None,
    )
    .0
}

fn run_benchmark_profiled_inner(
    words: &[u16],
    maximum_cycles: usize,
    trace_directory: Option<&Path>,
    pause_at: &mut impl FnMut(usize, u32) -> bool,
    start_from: Option<&Checkpoint>,
    mut checkpoint_at: Option<CheckpointFn<'_>>,
) -> (BenchResult, Vec<u16>, Option<Checkpoint>) {
    let mut memory = vec![0u16; BENCHMARK_SDRAM_WORDS];
    for (offset, word) in words.iter().copied().enumerate() {
        memory[offset] = word;
    }
    // Starting from a checkpoint restores the SDRAM image and controller phase,
    // not just the module states.
    let start_sdram = start_from.map(|checkpoint| checkpoint.snapshot.sdram.clone());
    if let Some(checkpoint) = start_from {
        memory.clone_from(&checkpoint.snapshot.memory);
    }

    let fetch_state = std::rc::Rc::new(std::cell::RefCell::new(
        cpu_v3::CpuV3InstructionFetchQueueState::default(),
    ));
    // State handles for every emulated module, so a complete machine snapshot can
    // be taken and restored (design/fetch-pause section 7.1).
    let core_state: CoreHandle =
        std::rc::Rc::new(std::cell::RefCell::new(cpu_v3::CpuV3CoreState::default()));
    let icache_state: IcacheHandle = std::rc::Rc::new(std::cell::RefCell::new(
        cpu_v3::CpuV3TwoWayCacheState::default(),
    ));
    let dcache_state: DcacheHandle = std::rc::Rc::new(std::cell::RefCell::new(
        cpu_v3::CpuV3DataCacheState::default(),
    ));
    let arbiter_state: ArbiterHandle =
        std::rc::Rc::new(std::cell::RefCell::new(CpuV3MemoryArbiterState::default()));
    let mut handles: Option<SystemHandles> = None;
    // Emulator-side fetch pause, owned by the system-control device model (see
    // `ObservedFetch`). Shared into the circuit closure so tests can toggle it.
    let fetch_pause = std::rc::Rc::new(std::cell::Cell::new(false));
    let (mut circuit, circuit_handles) = build_circuit(|| {
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
        dcache_input.line_copy_start = core_output.data_line_copy_valid;
        dcache_input.line_copy_source = core_output.data_line_copy_source;
        dcache_input.line_copy_destination_segment = core_output.data_line_copy_destination_segment;
        core_input.data_line_copy_ready = dcache_output.line_copy_ready;
        // Hold the core while the D-cache RAM16 valid arrays sweep-clear,
        // mirroring the system template's `sysctl_cpu_hold || valid_sweep`.
        core_input.hold = dcache_output.valid_sweep;

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
        // Every module is attached through `ObservedModule` rather than
        // `emu_connect` so the harness keeps a state handle it can snapshot.
        digital_design_circuit::external(ObservedModule::<CpuV3Core>::new(
            core_state.clone(),
            core_input.clone(),
            core_output.clone(),
        ));
        digital_design_circuit::external(ObservedFetch {
            state: fetch_state.clone(),
            input: fetch_input.clone(),
            output: fetch_output.clone(),
        });
        digital_design_circuit::external(ObservedModule::<CpuV3TwoWayCache>::new(
            icache_state.clone(),
            icache_input.clone(),
            icache_output.clone(),
        ));
        digital_design_circuit::external(ObservedModule::<CpuV3DataCache>::new(
            dcache_state.clone(),
            dcache_input.clone(),
            dcache_output.clone(),
        ));
        digital_design_circuit::external(ObservedModule::<CpuV3MemoryArbiter>::new(
            arbiter_state.clone(),
            arbiter_input.clone(),
            arbiter_output.clone(),
        ));
        handles = Some(SystemHandles {
            core: core_state.clone(),
            fetch: fetch_state.clone(),
            icache: icache_state.clone(),
            dcache: dcache_state.clone(),
            arbiter: arbiter_state.clone(),
        });

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
    ) = circuit_handles;

    // Restore a checkpoint before the run starts, so the prefix it covers is
    // reused rather than re-executed.
    let live_handles = handles.expect("the circuit closure must publish its state handles");
    let mut taken_checkpoint = None;
    if let Some(checkpoint) = start_from {
        live_handles.restore(&checkpoint.snapshot);
    }

    let mut sdram = start_sdram.unwrap_or_else(|| SdramModel::new(memory));
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
    let mut fetch_pause_cycles = 0u32;
    let mut retired_words_at_first_pause = None;
    let mut pause_requested = false;
    let mut data_side_busy_cycles = 0u32;
    let mut data_side_quiet_at = None;
    let mut halt_at = None;
    let mut halt_signal = 0u16;
    let mut flush_request = false;

    // Constant external inputs (device reads zero, DMA idle, no flush/invalidate).
    set_bits(core_input.device_read_data, 0, &mut circuit);
    // core_input.hold is connected to the D-cache valid sweep in the wiring above.
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

        // A test-supplied pause request (or the device's channel-6 register) is
        // evaluated BEFORE the settle passes so that it takes effect in the very
        // cycle it is asked for. Applying it after the passes would let the core
        // advance one more instruction and then gate it mid-handshake, which
        // never resumes.
        if !reset && halt_at.is_none() {
            pause_requested |= pause_at(cycle, previous_retired);
        }
        fetch_pause.set(pause_requested);
        let handshake_low = fetch_pause.get();
        set_bit(
            core_input.instruction_response_valid,
            !handshake_low && fetch_output.sample(&circuit).core_response_valid,
            &mut circuit,
        );
        set_bit(
            core_input.instruction_request_ready,
            !handshake_low && fetch_output.sample(&circuit).core_request_ready,
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

        // A pause request is sticky: it is honoured at the first cycle where the
        // machine is settled rather than dropped if it arrives during the D-cache
        // valid sweep (`valid_sweep` drives the core's `hold` through a gate, so
        // freezing then would hold the core forever) or while the core is
        // halting. The gate itself was already applied before the settle passes;
        // this block only keeps the request state up to date.
        if reset {
            pause_requested = false;
        } else if core.device_index == u64::from(SYSTEM_CONTROL_DEVICE)
            && core.device_write_enable
            && core.device_channel as u8 == FETCH_PAUSE_CHANNEL
        {
            pause_requested = core.device_write_data != 0;
        }

        if fetch_pause.get() {
            fetch_pause_cycles = fetch_pause_cycles.wrapping_add(1);
            if retired_words_at_first_pause.is_none() {
                retired_words_at_first_pause = Some(core.retired_words as u32);
            }
        }

        // Data-side quiescence: with the pause held the core issues no new data
        // request, so once the D-cache is accepting requests again every write
        // that retired before the pause has drained and the architectural memory
        // is stable. Record the first such cycle while paused.
        let data_side_busy = core.data_request_valid || !dcache.cpu_request_ready;
        if data_side_busy {
            data_side_busy_cycles = data_side_busy_cycles.wrapping_add(1);
        } else if fetch_pause.get() {
            if data_side_quiet_at.is_none() {
                data_side_quiet_at = Some(cycle);
            }
            // Paused and quiescent: the architectural state is stable, so this is
            // a point a checkpoint may be taken from. The observer keeps being
            // offered the machine afterwards, which lets a caller pick a point
            // (for example one where the SDRAM is still mid-transaction) rather
            // than being forced to accept the first quiet cycle.
            if taken_checkpoint.is_none() {
                if let Some(observer) = checkpoint_at.as_mut() {
                    let sdram_busy = sdram.state != SdramState::Idle;
                    if observer(cycle, &live_handles, sdram_busy) {
                        taken_checkpoint = Some(Checkpoint {
                            snapshot: live_handles.snapshot(
                                sdram.memory().to_vec(),
                                sdram.clone(),
                                fetch_pause.get(),
                            ),
                        });
                    }
                }
            }
        }

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
                    // ISA 0.8: major B relative forms (0..=7) and register
                    // jumps (E/F) can redirect; the conditional moves (8..D)
                    // cannot. JSEG is major 6 function F.
                    let can_redirect = (opcode == 0xbu16 && !matches!(function, 8u16..=13))
                        || (opcode == 0x6u16 && function == 15u16);
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

        // The machine keeps running while paused: only the frontend is held, so
        // the execute side drains and settles on its own and nothing is frozen
        // mid-transaction.
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
            write_btc_diagnostics(trace_directory, fetch_state.borrow().btc_statistics());
            halt_signal = core.halt_signal as u16;
            flush_request = true;
        } else if let Some(main_cycles) = halt_at {
            if dcache.maintenance_error {
                panic!("D-cache flush failed after benchmark completion");
            }
            if dcache.maintenance_done {
                let result = BenchResult {
                    program_words: words.len(),
                    cycles: main_cycles,
                    halt_signal,
                    retired_instructions,
                    retired_words: core.retired_words as u32,
                    // The I-cache prefetch mechanism was removed; the frozen
                    // suite CSV schema keeps these columns, pinned to zero.
                    prefetch_issued: 0,
                    prefetch_useful: 0,
                    prefetch_useless: 0,
                    prefetch_dropped: 0,
                    fetch_wait_cycles,
                    execute_cycles,
                    data_request_cycles,
                    data_response_cycles,
                    instruction_fetches,
                    icache_demand_requests,
                    data_requests,
                    icache_line_requests,
                    icache_demand_refills: icache_line_requests,
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
                    fetch_pause_cycles,
                    retired_words_at_first_pause,
                    data_side_busy_cycles,
                    data_side_quiet_at,
                };
                trace.summary(trace_directory, &result);
                return (result, sdram.memory().to_vec(), taken_checkpoint.take());
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
    pub dcache_maintenance_busy: bool,
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
            dcache_maintenance_busy: false,
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
    let mut memory = vec![0u16; SYSTEM_COSIM_SDRAM_WORDS];
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
        dcache_input.line_copy_start = core_output.data_line_copy_valid;
        dcache_input.line_copy_source = core_output.data_line_copy_source;
        dcache_input.line_copy_destination_segment = core_output.data_line_copy_destination_segment;
        core_input.data_line_copy_ready = dcache_output.line_copy_ready;
        // Hold the core while the D-cache RAM16 valid arrays sweep-clear,
        // mirroring the system template's `sysctl_cpu_hold || valid_sweep`.
        core_input.hold = dcache_output.valid_sweep;

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
    // core_input.hold is connected to the D-cache valid sweep in the wiring above.
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
            let mut observed = SystemCosimOut::from(&core);
            observed.dcache_maintenance_busy = dcache.maintenance_busy;
            cycles.push(observed);
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
