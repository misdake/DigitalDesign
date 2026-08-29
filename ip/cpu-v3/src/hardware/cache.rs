//! CpuV3 two-way physical-address cache with write-through stores.
//!
//! A read miss issues one aligned line request and captures the eight ordered
//! 32-bit response beats in a private 256-bit refill buffer. The cache then
//! drains eight 32-bit beats from the buffer into parity/way-interleaved data BSRAMs
//! and commits tag and valid state only after a complete error-free line, so
//! an error or invalidate can never expose a partially installed line. Writes
//! remain single write-through word transactions.

use digital_design_circuit::{CircuitWires, Wire, Wires};
use digital_design_hardware::{
    resources::components::{BsramBlocks, SsramBits},
    HardwareIdentity, Module, ModuleIo, TargetResourceRequest, VerilogDependency, VerilogIdentity,
};
use digital_design_hardware_gowin::BsramImage;
use digital_design_hardware_gowin::ZeroBsramImage;
use std::fmt::Write;
use std::marker::PhantomData;

pub const CPU_V3_CACHE_WAYS: usize = 2;
pub const CPU_V3_CACHE_WORDS_PER_WAY: usize = 1024;
pub const CPU_V3_CACHE_WORDS: usize = CPU_V3_CACHE_WAYS * CPU_V3_CACHE_WORDS_PER_WAY;
pub const CPU_V3_CACHE_LINE_WORDS: usize = 16;
pub const CPU_V3_CACHE_LINE_BEATS: usize = CPU_V3_CACHE_LINE_WORDS / 2;
pub const CPU_V3_CACHE_SETS: usize = CPU_V3_CACHE_WORDS_PER_WAY / CPU_V3_CACHE_LINE_WORDS;

/// Bank `b` contains words where `way XOR word_parity == b`. The initialized
/// image occupies way zero, so its even words land in bank zero and its odd
/// words land in bank one; the upper half reserved for way one starts clear.
const fn interleaved_bank_image<I: CpuV3CacheImage, const BANK: bool>(
) -> [u64; CPU_V3_CACHE_WORDS_PER_WAY] {
    let mut words = [0; CPU_V3_CACHE_WORDS_PER_WAY];
    let mut address = 0;
    while address < CPU_V3_CACHE_WORDS_PER_WAY / 2 {
        words[address] = I::WORDS[2 * address + BANK as usize];
        address += 1;
    }
    words
}

fn cache_data_image_hash<I: CpuV3CacheImage>() -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in b"cpu-v3-parity-way-interleaved-cache-data-v2"
        .iter()
        .copied()
        .chain([0])
        .chain(I::WORDS.iter().flat_map(|word| word.to_le_bytes()))
    {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

#[derive(Clone, ModuleIo)]
struct CpuV3ParitySplitCacheDataInput {
    bank_0_read_address: Wires<10>,
    bank_1_read_address: Wires<10>,
    bank_0_write_enable: Wire,
    bank_1_write_enable: Wire,
    write_address: Wires<10>,
    bank_0_write_data: Wires<16>,
    bank_1_write_data: Wires<16>,
}

#[derive(Clone, ModuleIo)]
struct CpuV3ParitySplitCacheDataOutput {
    bank_0_read_data: Wires<16>,
    bank_1_read_data: Wires<16>,
}

struct CpuV3ParitySplitCacheData<I>(PhantomData<I>);

impl<I: CpuV3CacheImage> HardwareIdentity for CpuV3ParitySplitCacheData<I> {
    const TARGET_RESOURCE_LEAF: bool = true;

    fn verilog_identity() -> VerilogIdentity {
        VerilogIdentity::new("CpuV3WayInterleavedCacheData")
            .namespace(["components", "cpu", "cpu_v3"])
            .symbol("IMAGE", format!("h{:016x}", cache_data_image_hash::<I>()))
    }
}

impl<I: CpuV3CacheImage> Module for CpuV3ParitySplitCacheData<I> {
    type Input = CpuV3ParitySplitCacheDataInput;
    type Output = CpuV3ParitySplitCacheDataOutput;
    type EmuState = ();

    const USES_MAIN_CLOCK: bool = true;
    const EMU_AVAILABLE: bool = false;

    fn target_resources() -> Vec<TargetResourceRequest> {
        vec![TargetResourceRequest::new(BsramBlocks::new(2))]
    }

    fn execute_emu(
        _state: &mut Self::EmuState,
        _circuit: &mut CircuitWires,
        _input: &Self::Input,
        _output: &Self::Output,
    ) {
        panic!("way-interleaved cache data BSRAM is Verilog-only")
    }

    fn verilog_source() -> Option<String> {
        let module_name = Self::verilog_identity().module_name();
        let bank_0 = interleaved_bank_image::<I, false>();
        let bank_1 = interleaved_bank_image::<I, true>();
        let mut overrides = String::new();
        for address in 0..CPU_V3_CACHE_WORDS_PER_WAY {
            assert!(bank_0[address] <= u64::from(u16::MAX));
            assert!(bank_1[address] <= u64::from(u16::MAX));
            if bank_0[address] != 0 {
                writeln!(
                    overrides,
                    "    bank_0_memory[10'd{address}] = 16'h{:04x};",
                    bank_0[address]
                )
                .unwrap();
            }
            if bank_1[address] != 0 {
                writeln!(
                    overrides,
                    "    bank_1_memory[10'd{address}] = 16'h{:04x};",
                    bank_1[address]
                )
                .unwrap();
            }
        }
        Some(format!(
            r#"module {module_name}(
    input wire clk,
    input wire [9:0] bank_0_read_address,
    input wire [9:0] bank_1_read_address,
    input wire bank_0_write_enable,
    input wire bank_1_write_enable,
    input wire [9:0] write_address,
    input wire [15:0] bank_0_write_data,
    input wire [15:0] bank_1_write_data,
    output reg [15:0] bank_0_read_data,
    output reg [15:0] bank_1_read_data
);

reg [15:0] bank_0_memory [0:1023];
reg [15:0] bank_1_memory [0:1023];
integer init_address;

initial begin
    for (init_address = 0; init_address < 1024; init_address = init_address + 1) begin
        bank_0_memory[init_address] = 16'h0000;
        bank_1_memory[init_address] = 16'h0000;
    end
{overrides}end

always @(posedge clk) begin
    bank_0_read_data <= bank_0_memory[bank_0_read_address];
    if (bank_0_write_enable)
        bank_0_memory[write_address] <= bank_0_write_data;
end

always @(posedge clk) begin
    bank_1_read_data <= bank_1_memory[bank_1_read_address];
    if (bank_1_write_enable)
        bank_1_memory[write_address] <= bank_1_write_data;
end

endmodule
"#
        ))
    }

    fn verilog_testbench() -> Option<String> {
        Some(
            include_str!("cpu_v3_way_interleaved_cache_data_tb.v").replace(
                "CpuV3WayInterleavedCacheData dut",
                &format!("{} dut", Self::verilog_identity().module_name()),
            ),
        )
    }
}

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
    CPU_V3_CACHE_WAYS * CPU_V3_CACHE_SETS.div_ceil(16) * CPU_V3_CACHE_TAG_BITS.div_ceil(4);
const CPU_V3_CACHE_TAG_PHYSICAL_BITS: usize = CPU_V3_CACHE_TAG_RAM16S * 64;

#[derive(Clone, ModuleIo)]
pub struct CpuV3CacheTagRamInput {
    pub write_enable: Wire,
    pub write_way: Wire,
    pub address: Wires<6>,
    pub write_data: Wires<12>,
}

#[derive(Clone, ModuleIo)]
pub struct CpuV3CacheTagRamOutput {
    pub way_0_read_data: Wires<12>,
    pub way_1_read_data: Wires<12>,
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
    type EmuState = [[u16; CPU_V3_CACHE_SETS]; CPU_V3_CACHE_WAYS];

    const USES_MAIN_CLOCK: bool = true;

    fn target_resources() -> Vec<TargetResourceRequest> {
        vec![TargetResourceRequest::new(SsramBits::new(
            CPU_V3_CACHE_TAG_PHYSICAL_BITS as u64,
        ))]
    }

    fn create_emu(_input: &Self::Input, _output: &Self::Output) -> Self::EmuState {
        [[0; CPU_V3_CACHE_SETS]; CPU_V3_CACHE_WAYS]
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
                way_0_read_data: u64::from(state[0][input.address as usize]),
                way_1_read_data: u64::from(state[1][input.address as usize]),
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
            state[input.write_way as usize][input.address as usize] = input.write_data as u16;
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
        VerilogIdentity::new("CpuV3TwoWayCache")
            .namespace(["components", "cpu", "cpu_v3"])
            .symbol(
                "IMAGE",
                format!(
                    "{}_v{:016x}",
                    CpuV3ParitySplitCacheData::<I>::verilog_identity().module_name(),
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
    tags: [[u16; CPU_V3_CACHE_SETS]; CPU_V3_CACHE_WAYS],
    valid: [[bool; CPU_V3_CACHE_SETS]; CPU_V3_CACHE_WAYS],
    victim: [usize; CPU_V3_CACHE_SETS],
    pending_way: usize,
    phase: Phase,
    pending: Pending,
    refill_buffer: [u32; CPU_V3_CACHE_LINE_BEATS],
    refill_beat: u8,
    drain_beat: u8,
    response_data: u16,
    response_error: bool,
}

impl Default for CpuV3DirectMappedCacheState {
    fn default() -> Self {
        Self {
            data: Box::new([0; CPU_V3_CACHE_WORDS]),
            tags: [[0; CPU_V3_CACHE_SETS]; CPU_V3_CACHE_WAYS],
            valid: [[false; CPU_V3_CACHE_SETS]; CPU_V3_CACHE_WAYS],
            victim: [0; CPU_V3_CACHE_SETS],
            pending_way: 0,
            phase: Phase::Idle,
            pending: Pending::default(),
            refill_buffer: [0; CPU_V3_CACHE_LINE_BEATS],
            refill_beat: 0,
            drain_beat: 0,
            response_data: 0,
            response_error: false,
        }
    }
}

impl CpuV3DirectMappedCacheState {
    fn initialized<I: CpuV3CacheImage>() -> Self {
        let mut state = Self::default();
        for (target, source) in state.data[..CPU_V3_CACHE_WORDS_PER_WAY]
            .iter_mut()
            .zip(I::WORDS)
        {
            *target = source as u16;
        }
        for set in 0..CPU_V3_CACHE_SETS {
            state.valid[0][set] = I::INITIAL_VALID & (1u64 << set) != 0;
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
                let hit_way = (0..CPU_V3_CACHE_WAYS)
                    .find(|way| state.valid[*way][set] && state.tags[*way][set] == tag);
                if state.pending.write {
                    if let Some(way) = hit_way {
                        state.data[data_index(way, set, word)] = state.pending.write_data;
                    }
                    state.phase = Phase::WordRequest;
                } else if let Some(way) = hit_way {
                    state.response_data = state.data[data_index(way, set, word)];
                    state.phase = Phase::CpuResponse;
                } else {
                    state.pending_way = (0..CPU_V3_CACHE_WAYS)
                        .find(|way| !state.valid[*way][set])
                        .unwrap_or(state.victim[set]);
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
                        state.drain_beat = 0;
                        state.phase = Phase::LineDrain;
                    } else {
                        state.refill_beat += 1;
                    }
                }
            }
            Phase::LineDrain => {
                let (set, tag, requested_word) = decode(state.pending.address);
                let drain = usize::from(state.drain_beat);
                let beat = state.refill_buffer[drain];
                let even_word = 2 * drain;
                state.data[data_index(state.pending_way, set, even_word)] = beat as u16;
                state.data[data_index(state.pending_way, set, even_word + 1)] = (beat >> 16) as u16;
                if drain == requested_word / 2 {
                    state.response_data = if requested_word % 2 == 0 {
                        beat as u16
                    } else {
                        (beat >> 16) as u16
                    };
                }
                if drain + 1 == CPU_V3_CACHE_LINE_BEATS {
                    state.tags[state.pending_way][set] = tag;
                    state.valid[state.pending_way][set] = true;
                    state.victim[set] = 1 - state.pending_way;
                    state.phase = Phase::CpuResponse;
                } else {
                    state.drain_beat += 1;
                }
            }
            Phase::CpuResponse if input.cpu_response_ready => state.phase = Phase::Idle,
            _ => {}
        }

        if input.invalidate_all {
            state.valid.fill([false; CPU_V3_CACHE_SETS]);
        }
    }

    fn verilog_source() -> Option<String> {
        let module_name = Self::verilog_identity().module_name();
        Some(
            include_str!("cpu_v3_two_way_cache.v")
                .replace(
                    "module CpuV3TwoWayCache (",
                    &format!("module {module_name} ("),
                )
                .replace(
                    "__INITIAL_VALID__",
                    &format!("64'h{:016x}", I::INITIAL_VALID),
                )
                .replace(
                    "__CACHE_DATA_BANKS__",
                    &CpuV3ParitySplitCacheData::<I>::verilog_identity().module_name(),
                )
                .replace(
                    "__CACHE_TAGS__",
                    &CpuV3CacheTagRam::verilog_identity().module_name(),
                ),
        )
    }

    fn verilog_dependencies() -> Vec<VerilogDependency> {
        vec![
            VerilogDependency::new::<CpuV3ParitySplitCacheData<I>>("u_data_banks"),
            VerilogDependency::new::<CpuV3CacheTagRam>("u_tags"),
        ]
    }

    fn verilog_testbench() -> Option<String> {
        Some(include_str!("cpu_v3_two_way_cache_tb.v").replace(
            "CpuV3TwoWayCache dut",
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

const fn data_index(way: usize, set: usize, word: usize) -> usize {
    (way * CPU_V3_CACHE_SETS + set) * CPU_V3_CACHE_LINE_WORDS + word
}

#[cfg(test)]
mod tests {
    use super::*;
    use digital_design_circuit::{build_circuit, Circuit};
    use digital_design_hardware::{ResourceAmount, ResourceKind, VerilogProject};
    use std::collections::{HashMap, VecDeque};

    struct InterleavedImage;

    impl BsramImage<16> for InterleavedImage {
        const WORDS: [u64; CPU_V3_CACHE_WORDS_PER_WAY] = {
            let mut words = [0; CPU_V3_CACHE_WORDS_PER_WAY];
            let mut index = 0;
            while index < CPU_V3_CACHE_WORDS_PER_WAY {
                words[index] = index as u64;
                index += 1;
            }
            words
        };
    }

    impl CpuV3CacheImage for InterleavedImage {
        const INITIAL_VALID: u64 = 1;
    }

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
                    self.beats
                        .push_back((u32::from(low) | u32::from(high) << 16, error));
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
    fn cache_image_is_interleaved_by_way_xor_parity_and_initializes_only_way_zero() {
        for address in 0..CPU_V3_CACHE_WORDS_PER_WAY / 2 {
            assert_eq!(
                interleaved_bank_image::<InterleavedImage, false>()[address],
                (2 * address) as u64
            );
            assert_eq!(
                interleaved_bank_image::<InterleavedImage, true>()[address],
                (2 * address + 1) as u64
            );
        }
        assert!(interleaved_bank_image::<InterleavedImage, false>()
            [CPU_V3_CACHE_WORDS_PER_WAY / 2..]
            .iter()
            .all(|word| *word == 0));
        assert!(interleaved_bank_image::<InterleavedImage, true>()
            [CPU_V3_CACHE_WORDS_PER_WAY / 2..]
            .iter()
            .all(|word| *word == 0));
    }

    #[test]
    #[ignore = "explicit external simulation of the way-interleaved cache data banks"]
    fn verify_way_interleaved_cache_data_with_iverilog() {
        digital_design_hardware::verify_verilog_with_iverilog::<
            CpuV3ParitySplitCacheData<ZeroBsramImage>,
        >()
        .unwrap();
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
            (0x4567, false, 0)
        );
        for address in 0x920..0x930 {
            memory.words.insert(address, (0x3000 | address) as u16);
        }
        assert_eq!(
            transact(&mut circuit, &input, &output, &mut memory, false, 0x923, 0),
            (0x3923, false, 1)
        );
        assert_eq!(
            transact(&mut circuit, &input, &output, &mut memory, false, 0x523, 0),
            (0x2523, false, 0)
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
    fn export_claims_two_data_bsram_and_characterized_tag_ssram_leaves() {
        let project = VerilogProject::generate::<CpuV3DirectMappedCache>().unwrap();
        assert_eq!(project.resource_claims.len(), 2);
        assert_eq!(
            project.resource_claims[0].resources,
            [ResourceAmount::new(ResourceKind::Bsram18K, 2)]
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
    #[ignore = "explicit external simulation of the CpuV3 two-way cache"]
    fn verify_verilog_with_iverilog() {
        digital_design_hardware::verify_verilog_with_iverilog::<CpuV3DirectMappedCache>().unwrap();
    }
}
