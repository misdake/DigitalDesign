//! First CpuV3 physical-address cache: direct-mapped and write-through.
//!
//! A read miss issues one aligned line request and captures the eight ordered
//! 32-bit response beats in a private 256-bit refill buffer. The cache then
//! drains sixteen 16-bit words from the buffer into its data BSRAM on its own
//! and commits tag and valid state only after a complete error-free line, so
//! an error or invalidate can never expose a partially installed line. Writes
//! remain single write-through word transactions.

use digital_design_circuit::{CircuitWires, Wire, Wires};
use digital_design_hardware::{
    resources::components::SsramBits, HardwareIdentity, Module, ModuleIo, TargetResourceRequest,
    VerilogDependency, VerilogIdentity,
};
use digital_design_hardware_gowin::BsramImage;
use digital_design_hardware_gowin::{Bsram1R1Rw1024, ZeroBsramImage};
use std::marker::PhantomData;

pub const CPU_V3_CACHE_WORDS: usize = 1024;
pub const CPU_V3_CACHE_LINE_WORDS: usize = 16;
pub const CPU_V3_CACHE_LINE_BEATS: usize = CPU_V3_CACHE_LINE_WORDS / 2;
pub const CPU_V3_CACHE_SETS: usize = CPU_V3_CACHE_WORDS / CPU_V3_CACHE_LINE_WORDS;

type CacheData<I> = Bsram1R1Rw1024<16, I>;

/// Power-up contents for a normal writable cache. Initial lines describe
/// physical segment zero; later misses replace them through the ordinary
/// refill path.
pub trait CpuV3CacheImage: BsramImage<16> {
    const INITIAL_VALID: u64;
}

impl CpuV3CacheImage for ZeroBsramImage {
    const INITIAL_VALID: u64 = 0;
}

const CPU_V3_CACHE_TAG_BITS: usize = 12;
const CPU_V3_CACHE_TAG_RAM16S: usize =
    CPU_V3_CACHE_SETS.div_ceil(16) * CPU_V3_CACHE_TAG_BITS.div_ceil(4);
const CPU_V3_CACHE_TAG_PHYSICAL_BITS: usize = CPU_V3_CACHE_TAG_RAM16S * 64;

#[derive(Clone, ModuleIo)]
pub struct CpuV3CacheTagRamInput {
    pub write_enable: Wire,
    pub address: Wires<6>,
    pub write_data: Wires<12>,
}

#[derive(Clone, ModuleIo)]
pub struct CpuV3CacheTagRamOutput {
    pub read_data: Wires<12>,
}

/// Characterized synchronous-write, asynchronous-read tag SSRAM.
pub struct CpuV3CacheTagRam;

impl HardwareIdentity for CpuV3CacheTagRam {
    const TARGET_RESOURCE_LEAF: bool = true;

    fn verilog_identity() -> VerilogIdentity {
        VerilogIdentity::new("CpuV3CacheTagRam").namespace(["components", "cpu", "cpu_v3"])
    }
}

impl Module for CpuV3CacheTagRam {
    type Input = CpuV3CacheTagRamInput;
    type Output = CpuV3CacheTagRamOutput;
    type EmuState = [u16; CPU_V3_CACHE_SETS];

    const USES_MAIN_CLOCK: bool = true;

    fn target_resources() -> Vec<TargetResourceRequest> {
        vec![TargetResourceRequest::new(SsramBits::new(
            CPU_V3_CACHE_TAG_PHYSICAL_BITS as u64,
        ))]
    }

    fn create_emu(_input: &Self::Input, _output: &Self::Output) -> Self::EmuState {
        [0; CPU_V3_CACHE_SETS]
    }

    fn execute_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        input: &Self::Input,
        output: &Self::Output,
    ) {
        let input = input.sample(circuit);
        output.drive(
            circuit,
            &CpuV3CacheTagRamOutputValue {
                read_data: u64::from(state[input.address as usize]),
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
        if input.write_enable {
            state[input.address as usize] = input.write_data as u16;
        }
    }

    fn verilog_source() -> Option<String> {
        Some(include_str!("cpu_v3_cache_tag_ram.v").to_string())
    }

    fn verilog_testbench() -> Option<String> {
        Some(include_str!("cpu_v3_cache_tag_ram_tb.v").to_string())
    }
}

#[derive(Clone, ModuleIo)]
pub struct CpuV3DirectMappedCacheInput {
    pub reset: Wire,
    pub invalidate_all: Wire,
    pub cpu_request_valid: Wire,
    pub cpu_write: Wire,
    pub cpu_address: Wires<32>,
    pub cpu_write_data: Wires<16>,
    pub cpu_response_ready: Wire,
    pub memory_request_ready: Wire,
    pub memory_response_valid: Wire,
    pub memory_read_data: Wires<32>,
    pub memory_error: Wire,
}

#[derive(Clone, ModuleIo)]
pub struct CpuV3DirectMappedCacheOutput {
    pub cpu_request_ready: Wire,
    pub cpu_response_valid: Wire,
    pub cpu_read_data: Wires<16>,
    pub cpu_error: Wire,
    pub memory_request_valid: Wire,
    pub memory_write: Wire,
    pub memory_address: Wires<22>,
    pub memory_write_data: Wires<16>,
    pub memory_response_ready: Wire,
}

pub struct CpuV3DirectMappedCacheWithImage<I>(PhantomData<I>);
pub type CpuV3DirectMappedCache = CpuV3DirectMappedCacheWithImage<ZeroBsramImage>;

impl<I: CpuV3CacheImage> HardwareIdentity for CpuV3DirectMappedCacheWithImage<I> {
    const TARGET_RESOURCE_LEAF: bool = false;

    fn verilog_identity() -> VerilogIdentity {
        VerilogIdentity::new("CpuV3DirectMappedCache")
            .namespace(["components", "cpu", "cpu_v3"])
            .symbol(
                "IMAGE",
                format!(
                    "{}_v{:016x}",
                    CacheData::<I>::verilog_identity().module_name(),
                    I::INITIAL_VALID
                ),
            )
    }
}

#[derive(Clone, Copy, Default, Eq, PartialEq)]
enum Phase {
    #[default]
    Idle,
    Check,
    WordRequest,
    WordResponse,
    LineRequest,
    LineReceive,
    LineDrain,
    CpuResponse,
}

#[derive(Clone, Copy, Default)]
struct Pending {
    write: bool,
    address: u32,
    write_data: u16,
}

pub struct CpuV3DirectMappedCacheState {
    data: Box<[u16; CPU_V3_CACHE_WORDS]>,
    tags: [u16; CPU_V3_CACHE_SETS],
    valid: [bool; CPU_V3_CACHE_SETS],
    phase: Phase,
    pending: Pending,
    refill_buffer: [u32; CPU_V3_CACHE_LINE_BEATS],
    refill_beat: u8,
    drain_word: u8,
    response_data: u16,
    response_error: bool,
}

impl Default for CpuV3DirectMappedCacheState {
    fn default() -> Self {
        Self {
            data: Box::new([0; CPU_V3_CACHE_WORDS]),
            tags: [0; CPU_V3_CACHE_SETS],
            valid: [false; CPU_V3_CACHE_SETS],
            phase: Phase::Idle,
            pending: Pending::default(),
            refill_buffer: [0; CPU_V3_CACHE_LINE_BEATS],
            refill_beat: 0,
            drain_word: 0,
            response_data: 0,
            response_error: false,
        }
    }
}

impl CpuV3DirectMappedCacheState {
    fn initialized<I: CpuV3CacheImage>() -> Self {
        let mut state = Self::default();
        for (target, source) in state.data.iter_mut().zip(I::WORDS) {
            *target = source as u16;
        }
        for set in 0..CPU_V3_CACHE_SETS {
            state.valid[set] = I::INITIAL_VALID & (1u64 << set) != 0;
        }
        state
    }
}

impl<I: CpuV3CacheImage> Module for CpuV3DirectMappedCacheWithImage<I> {
    type Input = CpuV3DirectMappedCacheInput;
    type Output = CpuV3DirectMappedCacheOutput;
    type EmuState = CpuV3DirectMappedCacheState;

    const USES_MAIN_CLOCK: bool = true;

    fn create_emu(_input: &Self::Input, _output: &Self::Output) -> Self::EmuState {
        CpuV3DirectMappedCacheState::initialized::<I>()
    }

    fn execute_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        _input: &Self::Input,
        output: &Self::Output,
    ) {
        output.drive(
            circuit,
            &CpuV3DirectMappedCacheOutputValue {
                cpu_request_ready: state.phase == Phase::Idle,
                cpu_response_valid: state.phase == Phase::CpuResponse,
                cpu_read_data: u64::from(state.response_data),
                cpu_error: state.phase == Phase::CpuResponse && state.response_error,
                memory_request_valid: matches!(
                    state.phase,
                    Phase::WordRequest | Phase::LineRequest
                ),
                memory_write: state.pending.write,
                memory_address: u64::from(if state.pending.write {
                    state.pending.address & 0x003f_ffff
                } else {
                    line_base(state.pending.address)
                }),
                memory_write_data: u64::from(state.pending.write_data),
                memory_response_ready: matches!(
                    state.phase,
                    Phase::WordResponse | Phase::LineReceive
                ),
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
        if input.reset {
            *state = CpuV3DirectMappedCacheState::initialized::<I>();
            return;
        }

        match state.phase {
            Phase::Idle if input.cpu_request_valid => {
                state.pending = Pending {
                    write: input.cpu_write,
                    address: input.cpu_address as u32,
                    write_data: input.cpu_write_data as u16,
                };
                state.response_error = false;
                if input.cpu_address >> 22 != 0 {
                    state.response_data = 0;
                    state.response_error = true;
                    state.phase = Phase::CpuResponse;
                } else {
                    state.phase = Phase::Check;
                }
            }
            Phase::Check => {
                let (set, tag, word) = decode(state.pending.address);
                let hit = state.valid[set] && state.tags[set] == tag;
                if state.pending.write {
                    if hit {
                        state.data[set * CPU_V3_CACHE_LINE_WORDS + word] = state.pending.write_data;
                    }
                    state.phase = Phase::WordRequest;
                } else if hit {
                    state.response_data = state.data[set * CPU_V3_CACHE_LINE_WORDS + word];
                    state.phase = Phase::CpuResponse;
                } else {
                    state.refill_beat = 0;
                    state.phase = Phase::LineRequest;
                }
            }
            Phase::WordRequest if input.memory_request_ready => {
                state.phase = Phase::WordResponse;
            }
            Phase::WordResponse if input.memory_response_valid => {
                state.response_data = 0;
                state.response_error = input.memory_error;
                state.phase = Phase::CpuResponse;
            }
            Phase::LineRequest if input.memory_request_ready => {
                state.refill_beat = 0;
                state.phase = Phase::LineReceive;
            }
            Phase::LineReceive if input.memory_response_valid => {
                if input.memory_error {
                    state.response_data = 0;
                    state.response_error = true;
                    state.phase = Phase::CpuResponse;
                } else {
                    state.refill_buffer[usize::from(state.refill_beat)] =
                        input.memory_read_data as u32;
                    if state.refill_beat as usize + 1 == CPU_V3_CACHE_LINE_BEATS {
                        state.drain_word = 0;
                        state.phase = Phase::LineDrain;
                    } else {
                        state.refill_beat += 1;
                    }
                }
            }
            Phase::LineDrain => {
                let (set, tag, requested_word) = decode(state.pending.address);
                let drain = usize::from(state.drain_word);
                let beat = state.refill_buffer[drain / 2];
                let value = if drain % 2 == 0 {
                    beat as u16
                } else {
                    (beat >> 16) as u16
                };
                state.data[set * CPU_V3_CACHE_LINE_WORDS + drain] = value;
                if drain == requested_word {
                    state.response_data = value;
                }
                if drain + 1 == CPU_V3_CACHE_LINE_WORDS {
                    state.tags[set] = tag;
                    state.valid[set] = true;
                    state.phase = Phase::CpuResponse;
                } else {
                    state.drain_word += 1;
                }
            }
            Phase::CpuResponse if input.cpu_response_ready => state.phase = Phase::Idle,
            _ => {}
        }

        if input.invalidate_all {
            state.valid.fill(false);
        }
    }

    fn verilog_source() -> Option<String> {
        let module_name = Self::verilog_identity().module_name();
        Some(
            include_str!("cpu_v3_direct_mapped_cache.v")
                .replace(
                    "module CpuV3DirectMappedCache (",
                    &format!("module {module_name} ("),
                )
                .replace(
                    "__INITIAL_VALID__",
                    &format!("64'h{:016x}", I::INITIAL_VALID),
                )
                .replace(
                    "__CACHE_DATA__",
                    &CacheData::<I>::verilog_identity().module_name(),
                )
                .replace(
                    "__CACHE_TAGS__",
                    &CpuV3CacheTagRam::verilog_identity().module_name(),
                ),
        )
    }

    fn verilog_dependencies() -> Vec<VerilogDependency> {
        vec![
            VerilogDependency::new::<CacheData<I>>("u_data"),
            VerilogDependency::new::<CpuV3CacheTagRam>("u_tags"),
        ]
    }

    fn verilog_testbench() -> Option<String> {
        Some(include_str!("cpu_v3_direct_mapped_cache_tb.v").replace(
            "CpuV3DirectMappedCache dut",
            &format!("{} dut", Self::verilog_identity().module_name()),
        ))
    }
}

const fn line_base(address: u32) -> u32 {
    address & !((CPU_V3_CACHE_LINE_WORDS as u32) - 1) & 0x003f_ffff
}

const fn decode(address: u32) -> (usize, u16, usize) {
    let word = (address as usize) & (CPU_V3_CACHE_LINE_WORDS - 1);
    let set = ((address as usize) >> 4) & (CPU_V3_CACHE_SETS - 1);
    let tag = ((address >> 10) & 0x0fff) as u16;
    (set, tag, word)
}

#[cfg(test)]
mod tests {
    use super::*;
    use digital_design_circuit::{build_circuit, Circuit};
    use digital_design_hardware::{ResourceAmount, ResourceKind, VerilogProject};
    use std::collections::{HashMap, VecDeque};

    /// A line-serving memory model behind the cache port: a read request
    /// returns eight ordered 32-bit beats (low half = even word), a write
    /// request returns one completion beat.
    struct MemoryModel {
        words: HashMap<u32, u16>,
        beats: VecDeque<(u32, bool)>,
        requests: usize,
        /// Beat index within a line response that carries an error instead.
        error_on_beat: Option<usize>,
    }

    impl MemoryModel {
        fn new() -> Self {
            Self {
                words: HashMap::new(),
                beats: VecDeque::new(),
                requests: 0,
                error_on_beat: None,
            }
        }

        fn accept(&mut self, write: bool, address: u32, write_data: u16) {
            self.requests += 1;
            if write {
                self.words.insert(address, write_data);
                self.beats.push_back((0, false));
            } else {
                for beat in 0..CPU_V3_CACHE_LINE_BEATS as u32 {
                    let low = self.word(address + 2 * beat);
                    let high = self.word(address + 2 * beat + 1);
                    let error = self.error_on_beat == Some(beat as usize);
                    self.beats.push_back((u32::from(low) | u32::from(high) << 16, error));
                }
            }
        }

        fn word(&self, address: u32) -> u16 {
            self.words.get(&address).copied().unwrap_or(0)
        }
    }

    fn drive(
        circuit: &mut Circuit,
        input: &CpuV3DirectMappedCacheInput,
        cpu_request: Option<(bool, u32, u16)>,
        cpu_response_ready: bool,
        memory_response: Option<(u32, bool)>,
        invalidate_all: bool,
    ) {
        let (cpu_write, cpu_address, cpu_write_data) = cpu_request.unwrap_or_default();
        input.drive(
            circuit,
            &CpuV3DirectMappedCacheInputValue {
                reset: false,
                invalidate_all,
                cpu_request_valid: cpu_request.is_some(),
                cpu_write,
                cpu_address: u64::from(cpu_address),
                cpu_write_data: u64::from(cpu_write_data),
                cpu_response_ready,
                memory_request_ready: true,
                memory_response_valid: memory_response.is_some(),
                memory_read_data: u64::from(memory_response.unwrap_or_default().0),
                memory_error: memory_response.unwrap_or_default().1,
            },
        );
    }

    fn transact(
        circuit: &mut Circuit,
        input: &CpuV3DirectMappedCacheInput,
        output: &CpuV3DirectMappedCacheOutput,
        memory: &mut MemoryModel,
        write: bool,
        address: u32,
        write_data: u16,
    ) -> (u16, bool, usize) {
        let requests_before = memory.requests;
        let mut cpu_request = Some((write, address, write_data));
        for _ in 0..300 {
            drive(
                circuit,
                input,
                cpu_request,
                false,
                memory.beats.pop_front(),
                false,
            );
            circuit.execute_gates();
            let value = output.sample(circuit);
            if cpu_request.is_some() && value.cpu_request_ready {
                cpu_request = None;
            }
            if value.memory_request_valid {
                memory.accept(
                    value.memory_write,
                    value.memory_address as u32,
                    value.memory_write_data as u16,
                );
            }
            if value.cpu_response_valid {
                let result = (
                    value.cpu_read_data as u16,
                    value.cpu_error,
                    memory.requests - requests_before,
                );
                // An error-terminated line response drops the model's
                // unsent beats, exactly like the arbiter drops them.
                memory.beats.clear();
                drive(circuit, input, None, true, None, false);
                circuit.clock_tick();
                return result;
            }
            circuit.clock_tick();
        }
        panic!("cache transaction did not complete")
    }

    fn fixture() -> (
        Circuit,
        CpuV3DirectMappedCacheInput,
        CpuV3DirectMappedCacheOutput,
    ) {
        let (circuit, (input, output)) = build_circuit(|| {
            let input = CpuV3DirectMappedCacheInput::allocate();
            let output = CpuV3DirectMappedCache::emu(&input);
            (input, output)
        });
        (circuit, input, output)
    }

    #[test]
    fn miss_refills_one_line_through_eight_beats_and_hits_afterwards() {
        let (mut circuit, input, output) = fixture();
        let mut memory = MemoryModel::new();
        for address in 0x120..0x130 {
            memory.words.insert(address, (0x8000 | address) as u16);
        }
        assert_eq!(
            transact(&mut circuit, &input, &output, &mut memory, false, 0x123, 0),
            (0x8123, false, 1)
        );
        // Even and odd words come from the low and high beat halves.
        assert_eq!(
            transact(&mut circuit, &input, &output, &mut memory, false, 0x12e, 0),
            (0x812e, false, 0)
        );
        assert_eq!(
            transact(&mut circuit, &input, &output, &mut memory, false, 0x12f, 0),
            (0x812f, false, 0)
        );
    }

    #[test]
    fn write_through_conflict_and_full_invalidate_follow_physical_tags() {
        let (mut circuit, input, output) = fixture();
        let mut memory = MemoryModel::new();
        for address in 0x120..0x130 {
            memory.words.insert(address, (0x8000 | address) as u16);
        }
        transact(&mut circuit, &input, &output, &mut memory, false, 0x123, 0);
        assert_eq!(
            transact(
                &mut circuit,
                &input,
                &output,
                &mut memory,
                true,
                0x123,
                0x4567,
            ),
            (0, false, 1)
        );
        assert_eq!(memory.words[&0x123], 0x4567);
        assert_eq!(
            transact(&mut circuit, &input, &output, &mut memory, false, 0x123, 0),
            (0x4567, false, 0)
        );

        drive(&mut circuit, &input, None, false, None, true);
        circuit.clock_tick();
        assert_eq!(
            transact(&mut circuit, &input, &output, &mut memory, false, 0x123, 0),
            (0x4567, false, 1)
        );

        for address in 0x520..0x530 {
            memory.words.insert(address, (0x2000 | address) as u16);
        }
        assert_eq!(
            transact(&mut circuit, &input, &output, &mut memory, false, 0x523, 0),
            (0x2523, false, 1)
        );
        assert_eq!(
            transact(&mut circuit, &input, &output, &mut memory, false, 0x123, 0),
            (0x4567, false, 1)
        );
    }

    #[test]
    fn refill_error_aborts_without_installing_a_partial_line() {
        let (mut circuit, input, output) = fixture();
        let mut memory = MemoryModel::new();
        for address in 0x120..0x130 {
            memory.words.insert(address, (0x8000 | address) as u16);
        }
        memory.error_on_beat = Some(3);
        assert_eq!(
            transact(&mut circuit, &input, &output, &mut memory, false, 0x123, 0),
            (0, true, 1)
        );
        // The failed line never became valid: the next read misses again and
        // the memory observes one more line request.
        memory.error_on_beat = None;
        assert_eq!(
            transact(&mut circuit, &input, &output, &mut memory, false, 0x123, 0),
            (0x8123, false, 1)
        );
    }

    #[test]
    fn address_beyond_fitted_physical_memory_faults_without_downstream_io() {
        let (mut circuit, input, output) = fixture();
        let mut memory = MemoryModel::new();
        assert_eq!(
            transact(
                &mut circuit,
                &input,
                &output,
                &mut memory,
                false,
                0x0040_0000,
                0,
            ),
            (0, true, 0)
        );
    }

    #[test]
    fn export_claims_data_bsram_and_characterized_tag_ssram_leaves() {
        let project = VerilogProject::generate::<CpuV3DirectMappedCache>().unwrap();
        assert_eq!(project.resource_claims.len(), 2);
        assert_eq!(
            project.resource_claims[0].resources,
            [ResourceAmount::new(ResourceKind::Bsram18K, 1)]
        );
        assert_eq!(
            project.resource_claims[1].resources,
            [ResourceAmount::new(
                ResourceKind::SsramBit,
                CPU_V3_CACHE_TAG_PHYSICAL_BITS as u64,
            )]
        );
    }

    #[test]
    #[ignore = "explicit external simulation of the CpuV3 direct-mapped cache"]
    fn verify_verilog_with_iverilog() {
        digital_design_hardware::verify_verilog_with_iverilog::<CpuV3DirectMappedCache>().unwrap();
    }
}
