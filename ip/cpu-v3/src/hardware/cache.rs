//! CpuV3 two-way physical-address cache hardware.
//!
//! Cache lines use four ordered 64-bit memory beats. Two true-dual-port data
//! BSRAMs split each line by word parity and directly transfer all four words
//! of a beat. Tags and valid bits commit only after a complete error-free line. The
//! production I-cache exposes a read-only boundary around this refill/prefetch
//! engine; the independent D-cache implements write-allocate, dirty eviction,
//! and blocking global clean/invalidate.

use digital_design_circuit::{CircuitWires, Wire, Wires};
use digital_design_hardware::{
    resources::components::{BsramBlocks, SsramBits},
    HardwareIdentity, Module, ModuleIo, TargetResourceRequest, VerilogDependency, VerilogIdentity,
};
use digital_design_hardware_gowin::{BsramImage, ZeroBsramImage};
use std::fmt::Write;
use std::marker::PhantomData;

pub const CPU_V3_CACHE_WAYS: usize = 2;
pub const CPU_V3_CACHE_WORDS_PER_WAY: usize = 1024;
pub const CPU_V3_CACHE_WORDS: usize = CPU_V3_CACHE_WAYS * CPU_V3_CACHE_WORDS_PER_WAY;
pub const CPU_V3_CACHE_LINE_WORDS: usize = 16;
pub const CPU_V3_CACHE_LINE_BEATS: usize = CPU_V3_CACHE_LINE_WORDS / 2;
pub const CPU_V3_CACHE_MEMORY_BEATS: usize = CPU_V3_CACHE_LINE_WORDS / 4;
pub const CPU_V3_CACHE_SETS: usize = CPU_V3_CACHE_WORDS_PER_WAY / CPU_V3_CACHE_LINE_WORDS;

/// Bank `b` contains words whose word parity is `b`. Way zero occupies the
/// lower 512 addresses and way one occupies the upper 512 addresses.
const fn parity_bank_image<I: CpuV3CacheImage, const BANK: bool>() -> [u64; 1024] {
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
    for byte in b"cpu-v3-parity-split-true-dual-port-cache-data-v1"
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
struct CpuV3DualPortCacheDataInput {
    bank_0_a_write_enable: Wire,
    bank_0_a_address: Wires<10>,
    bank_0_a_write_data: Wires<16>,
    bank_0_b_write_enable: Wire,
    bank_0_b_address: Wires<10>,
    bank_0_b_write_data: Wires<16>,
    bank_1_a_write_enable: Wire,
    bank_1_a_address: Wires<10>,
    bank_1_a_write_data: Wires<16>,
    bank_1_b_write_enable: Wire,
    bank_1_b_address: Wires<10>,
    bank_1_b_write_data: Wires<16>,
}

#[derive(Clone, ModuleIo)]
struct CpuV3DualPortCacheDataOutput {
    bank_0_a_read_data: Wires<16>,
    bank_0_b_read_data: Wires<16>,
    bank_1_a_read_data: Wires<16>,
    bank_1_b_read_data: Wires<16>,
}

struct CpuV3DualPortCacheData<I>(PhantomData<I>);

impl<I: CpuV3CacheImage> HardwareIdentity for CpuV3DualPortCacheData<I> {
    const TARGET_RESOURCE_LEAF: bool = true;

    fn verilog_identity() -> VerilogIdentity {
        VerilogIdentity::new("CpuV3DualPortCacheData")
            .namespace(["components", "cpu", "cpu_v3"])
            .symbol("IMAGE", format!("h{:016x}", cache_data_image_hash::<I>()))
    }
}

impl<I: CpuV3CacheImage> Module for CpuV3DualPortCacheData<I> {
    type Input = CpuV3DualPortCacheDataInput;
    type Output = CpuV3DualPortCacheDataOutput;
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
        panic!("dual-port cache data BSRAM is Verilog-only")
    }

    fn verilog_source() -> Option<String> {
        let module_name = Self::verilog_identity().module_name();
        let bank_0 = parity_bank_image::<I, false>();
        let bank_1 = parity_bank_image::<I, true>();
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
    input wire bank_0_a_write_enable, input wire [9:0] bank_0_a_address,
    input wire [15:0] bank_0_a_write_data, output wire [15:0] bank_0_a_read_data,
    input wire bank_0_b_write_enable, input wire [9:0] bank_0_b_address,
    input wire [15:0] bank_0_b_write_data, output wire [15:0] bank_0_b_read_data,
    input wire bank_1_a_write_enable, input wire [9:0] bank_1_a_address,
    input wire [15:0] bank_1_a_write_data, output wire [15:0] bank_1_a_read_data,
    input wire bank_1_b_write_enable, input wire [9:0] bank_1_b_address,
    input wire [15:0] bank_1_b_write_data, output wire [15:0] bank_1_b_read_data
);
reg [15:0] bank_0_memory [0:1023];
reg [15:0] bank_1_memory [0:1023];
reg [15:0] bank_0_a_read_data_r, bank_0_b_read_data_r;
reg [15:0] bank_1_a_read_data_r, bank_1_b_read_data_r;
integer init_address;

assign bank_0_a_read_data = bank_0_a_read_data_r;
assign bank_0_b_read_data = bank_0_b_read_data_r;
assign bank_1_a_read_data = bank_1_a_read_data_r;
assign bank_1_b_read_data = bank_1_b_read_data_r;

initial begin
    for (init_address = 0; init_address < 1024; init_address = init_address + 1) begin
        bank_0_memory[init_address] = 16'h0000;
        bank_1_memory[init_address] = 16'h0000;
    end
{overrides}end

always @(posedge clk) begin
    if (bank_0_a_write_enable) bank_0_memory[bank_0_a_address] <= bank_0_a_write_data;
    else bank_0_a_read_data_r <= bank_0_memory[bank_0_a_address];
end
always @(posedge clk) begin
    if (bank_0_b_write_enable) bank_0_memory[bank_0_b_address] <= bank_0_b_write_data;
    else bank_0_b_read_data_r <= bank_0_memory[bank_0_b_address];
end
always @(posedge clk) begin
    if (bank_1_a_write_enable) bank_1_memory[bank_1_a_address] <= bank_1_a_write_data;
    else bank_1_a_read_data_r <= bank_1_memory[bank_1_a_address];
end
always @(posedge clk) begin
    if (bank_1_b_write_enable) bank_1_memory[bank_1_b_address] <= bank_1_b_write_data;
    else bank_1_b_read_data_r <= bank_1_memory[bank_1_b_address];
end
endmodule
"#
        ))
    }

    fn verilog_testbench() -> Option<String> {
        Some(include_str!("cpu_v3_dual_port_cache_data_tb.v").replace(
            "CpuV3DualPortCacheData dut",
            &format!("{} dut", Self::verilog_identity().module_name()),
        ))
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
pub struct CpuV3TwoWayCacheInput {
    pub reset: Wire,
    pub invalidate_all: Wire,
    pub prefetch_request_valid: Wire,
    pub prefetch_address: Wires<32>,
    pub prefetch_cancel: Wire,
    pub cpu_request_valid: Wire,
    pub cpu_write: Wire,
    pub cpu_address: Wires<32>,
    pub cpu_write_data: Wires<16>,
    pub cpu_response_ready: Wire,
    pub memory_request_ready: Wire,
    pub memory_response_valid: Wire,
    pub memory_read_data: Wires<64>,
    pub memory_error: Wire,
}

#[derive(Clone, ModuleIo)]
pub struct CpuV3TwoWayCacheOutput {
    pub cpu_request_ready: Wire,
    pub cpu_response_valid: Wire,
    pub cpu_read_data: Wires<16>,
    pub cpu_error: Wire,
    pub memory_request_valid: Wire,
    pub memory_write: Wire,
    pub memory_line: Wire,
    pub memory_address: Wires<22>,
    pub memory_write_data: Wires<64>,
    pub memory_response_ready: Wire,
    pub prefetch_issued: Wires<32>,
    pub prefetch_useful: Wires<32>,
    pub prefetch_useless: Wires<32>,
    pub prefetch_dropped: Wires<32>,
}

pub struct CpuV3TwoWayCacheWithImage<I>(PhantomData<I>);
pub type CpuV3TwoWayCache = CpuV3TwoWayCacheWithImage<ZeroBsramImage>;

#[derive(Clone, ModuleIo)]
pub struct CpuV3InstructionCacheInput {
    pub reset: Wire,
    pub invalidate_all: Wire,
    pub prefetch_request_valid: Wire,
    pub prefetch_address: Wires<32>,
    pub prefetch_cancel: Wire,
    pub cpu_request_valid: Wire,
    pub cpu_address: Wires<32>,
    pub cpu_response_ready: Wire,
    pub memory_request_ready: Wire,
    pub memory_response_valid: Wire,
    pub memory_read_data: Wires<64>,
    pub memory_error: Wire,
}

#[derive(Clone, ModuleIo)]
pub struct CpuV3InstructionCacheOutput {
    pub cpu_request_ready: Wire,
    pub cpu_response_valid: Wire,
    pub cpu_read_data: Wires<16>,
    pub cpu_error: Wire,
    pub memory_request_valid: Wire,
    pub memory_address: Wires<22>,
    pub memory_response_ready: Wire,
    pub prefetch_issued: Wires<32>,
    pub prefetch_useful: Wires<32>,
    pub prefetch_useless: Wires<32>,
    pub prefetch_dropped: Wires<32>,
}

/// Production read-only I-cache boundary. The proven refill/prefetch engine is
/// retained underneath, with its legacy store pins tied off so synthesis
/// removes the unreachable write-through path.
pub struct CpuV3InstructionCache;

impl HardwareIdentity for CpuV3InstructionCache {
    const TARGET_RESOURCE_LEAF: bool = false;

    fn verilog_identity() -> VerilogIdentity {
        VerilogIdentity::new("CpuV3InstructionCache").namespace(["components", "cpu", "cpu_v3"])
    }
}

impl Module for CpuV3InstructionCache {
    type Input = CpuV3InstructionCacheInput;
    type Output = CpuV3InstructionCacheOutput;
    type EmuState = ();

    const USES_MAIN_CLOCK: bool = true;
    const EMU_AVAILABLE: bool = false;

    fn execute_emu(
        _state: &mut Self::EmuState,
        _circuit: &mut CircuitWires,
        _input: &Self::Input,
        _output: &Self::Output,
    ) {
        panic!("the production I-cache wrapper is Verilog-only")
    }

    fn verilog_source() -> Option<String> {
        Some(include_str!("cpu_v3_instruction_cache.v").replace(
            "__CACHE__",
            &CpuV3TwoWayCache::verilog_identity().module_name(),
        ))
    }

    fn verilog_dependencies() -> Vec<VerilogDependency> {
        vec![VerilogDependency::new::<CpuV3TwoWayCache>("u_cache")]
    }

    fn verilog_testbench() -> Option<String> {
        Some(include_str!("cpu_v3_instruction_cache_tb.v").to_string())
    }
}

impl<I: CpuV3CacheImage> HardwareIdentity for CpuV3TwoWayCacheWithImage<I> {
    const TARGET_RESOURCE_LEAF: bool = false;

    fn verilog_identity() -> VerilogIdentity {
        VerilogIdentity::new("CpuV3TwoWayCache")
            .namespace(["components", "cpu", "cpu_v3"])
            .symbol(
                "IMAGE",
                format!(
                    "{}_v{:016x}",
                    CpuV3DualPortCacheData::<I>::verilog_identity().module_name(),
                    I::INITIAL_VALID
                ),
            )
    }
}

#[derive(Clone, Copy, Default, Eq, PartialEq, Debug)]
enum State {
    #[default]
    Idle,
    WordRequest,
    WordResponse,
    LineRequest,
    LineReceive,
}

#[derive(Clone, Copy, Default)]
struct Pending {
    is_prefetch: bool,
    write: bool,
    address: u32,
    write_data: u16,
}

#[derive(Clone)]
pub struct CpuV3TwoWayCacheState {
    data: Box<[u16; CPU_V3_CACHE_WORDS]>,
    tags: [[u16; CPU_V3_CACHE_SETS]; CPU_V3_CACHE_WAYS],
    valid: [[bool; CPU_V3_CACHE_SETS]; CPU_V3_CACHE_WAYS],
    prefetched: [[bool; CPU_V3_CACHE_SETS]; CPU_V3_CACHE_WAYS],
    victim: [usize; CPU_V3_CACHE_SETS],
    pending_way: usize,
    state: State,
    lookup_valid: bool,
    pending: Pending,
    refill_beat: u8,
    refill_response_data: u16,
    response_data: u16,
    response_error: bool,
    response_valid: bool,
    prefetch_pending: Option<u32>,
    refill_discard: bool,
    prefetch_armed: bool,
    prefetch_issued: u32,
    prefetch_useful: u32,
    prefetch_useless: u32,
    prefetch_dropped: u32,
}

impl Default for CpuV3TwoWayCacheState {
    fn default() -> Self {
        Self {
            data: Box::new([0; CPU_V3_CACHE_WORDS]),
            tags: [[0; CPU_V3_CACHE_SETS]; CPU_V3_CACHE_WAYS],
            valid: [[false; CPU_V3_CACHE_SETS]; CPU_V3_CACHE_WAYS],
            prefetched: [[false; CPU_V3_CACHE_SETS]; CPU_V3_CACHE_WAYS],
            victim: [0; CPU_V3_CACHE_SETS],
            pending_way: 0,
            state: State::Idle,
            lookup_valid: false,
            pending: Pending::default(),
            refill_beat: 0,
            refill_response_data: 0,
            response_data: 0,
            response_error: false,
            response_valid: false,
            prefetch_pending: None,
            refill_discard: false,
            prefetch_armed: false,
            prefetch_issued: 0,
            prefetch_useful: 0,
            prefetch_useless: 0,
            prefetch_dropped: 0,
        }
    }
}

impl CpuV3TwoWayCacheState {
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

impl<I: CpuV3CacheImage> Module for CpuV3TwoWayCacheWithImage<I> {
    type Input = CpuV3TwoWayCacheInput;
    type Output = CpuV3TwoWayCacheOutput;
    type EmuState = CpuV3TwoWayCacheState;

    const USES_MAIN_CLOCK: bool = true;

    fn create_emu(_input: &Self::Input, _output: &Self::Output) -> Self::EmuState {
        CpuV3TwoWayCacheState::initialized::<I>()
    }

    fn execute_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        input: &Self::Input,
        output: &Self::Output,
    ) {
        let input = input.sample(circuit);
        let (set, tag, _) = decode(state.pending.address);
        let pending_address_valid = state.pending.address >> 22 == 0;
        let way_0_hit = state.valid[0][set] && state.tags[0][set] == tag;
        let way_1_hit = state.valid[1][set] && state.tags[1][set] == tag;
        let pending_hit = way_0_hit || way_1_hit;
        let lookup_read_hit =
            state.lookup_valid && pending_address_valid && !state.pending.write && pending_hit;
        let response_space = !state.response_valid || input.cpu_response_ready;
        let steal =
            state.state == State::LineRequest && state.pending.is_prefetch && !state.prefetch_armed;
        let cpu_request_ready = !input.invalidate_all
            && ((state.state == State::Idle
                && (!state.lookup_valid
                    || (state.pending.is_prefetch || lookup_read_hit) && response_space))
                || steal);
        output.drive(
            circuit,
            &CpuV3TwoWayCacheOutputValue {
                cpu_request_ready,
                cpu_response_valid: state.response_valid,
                cpu_read_data: u64::from(state.response_data),
                cpu_error: state.response_valid && state.response_error,
                memory_request_valid: state.state == State::WordRequest
                    || (state.state == State::LineRequest
                        && (!state.pending.is_prefetch || state.prefetch_armed)),
                memory_write: state.pending.write,
                memory_line: !state.pending.write,
                memory_address: u64::from(if state.pending.write {
                    state.pending.address & 0x003f_ffff
                } else {
                    line_base(state.pending.address)
                }),
                memory_write_data: u64::from(state.pending.write_data),
                memory_response_ready: matches!(
                    state.state,
                    State::WordResponse | State::LineReceive
                ),
                prefetch_issued: u64::from(state.prefetch_issued),
                prefetch_useful: u64::from(state.prefetch_useful),
                prefetch_useless: u64::from(state.prefetch_useless),
                prefetch_dropped: u64::from(state.prefetch_dropped),
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
            *state = CpuV3TwoWayCacheState::initialized::<I>();
            return;
        }

        // Combinational values evaluated against the current (pre-edge) state.
        let (set, tag, word) = decode(state.pending.address);
        let pending_address_valid = state.pending.address >> 22 == 0;
        let way_0_hit = state.valid[0][set] && state.tags[0][set] == tag;
        let way_1_hit = state.valid[1][set] && state.tags[1][set] == tag;
        let pending_hit = way_0_hit || way_1_hit;
        let hit_way = if way_0_hit { 0 } else { 1 };
        let selected_victim = if !state.valid[0][set] {
            0
        } else if !state.valid[1][set] {
            1
        } else {
            state.victim[set]
        };
        let response_space = !state.response_valid || input.cpu_response_ready;
        let hit_write = state.state == State::Idle
            && state.lookup_valid
            && state.pending.write
            && pending_hit
            && response_space
            && !input.invalidate_all;
        let lookup_read_hit =
            state.lookup_valid && pending_address_valid && !state.pending.write && pending_hit;
        let cancel_prefetch = input.prefetch_cancel || input.invalidate_all;
        let prefetch_refill_cancelled = state.pending.is_prefetch && input.prefetch_cancel;
        let steal =
            state.state == State::LineRequest && state.pending.is_prefetch && !state.prefetch_armed;
        let cpu_request_ready = !input.invalidate_all
            && ((state.state == State::Idle
                && (!state.lookup_valid
                    || (state.pending.is_prefetch || lookup_read_hit) && response_space))
                || steal);
        let accept_cpu_request = input.cpu_request_valid && cpu_request_ready;
        let accept_prefetch = state.state == State::Idle
            && !input.invalidate_all
            && !cancel_prefetch
            && !state.lookup_valid
            && !state.response_valid
            && state.prefetch_pending.is_some()
            && !input.cpu_request_valid;

        let mut next = state.clone();

        if state.response_valid && input.cpu_response_ready {
            next.response_valid = false;
        }

        if !cancel_prefetch && input.prefetch_request_valid {
            let address = input.prefetch_address as u32;
            if state
                .prefetch_pending
                .is_some_and(|pending| pending != address)
            {
                next.prefetch_dropped = next.prefetch_dropped.wrapping_add(1);
            }
            next.prefetch_pending = Some(address);
        }

        match state.state {
            State::Idle => {
                if state.lookup_valid
                    && (state.pending.is_prefetch || response_space)
                    && !(cancel_prefetch && state.pending.is_prefetch)
                {
                    if !pending_address_valid {
                        if state.pending.is_prefetch {
                            next.prefetch_dropped = next.prefetch_dropped.wrapping_add(1);
                        } else {
                            next.response_data = 0;
                            next.response_error = true;
                            next.response_valid = true;
                        }
                        next.lookup_valid = false;
                        if state.pending.is_prefetch {
                            next.pending.is_prefetch = false;
                        }
                    } else if state.pending.write {
                        next.lookup_valid = false;
                        next.state = State::WordRequest;
                    } else if pending_hit {
                        if !state.pending.is_prefetch {
                            next.response_data = state.data[data_index(hit_way, set, word)];
                            next.response_error = false;
                            next.response_valid = true;
                            if hit_way == 1 && state.prefetched[1][set] {
                                next.prefetched[1][set] = false;
                                next.prefetch_useful = next.prefetch_useful.wrapping_add(1);
                            } else if hit_way == 0 && state.prefetched[0][set] {
                                next.prefetched[0][set] = false;
                                next.prefetch_useful = next.prefetch_useful.wrapping_add(1);
                            }
                        }
                        next.lookup_valid = false;
                        if state.pending.is_prefetch {
                            next.pending.is_prefetch = false;
                        }
                    } else {
                        next.lookup_valid = false;
                        if state.pending.is_prefetch && accept_cpu_request {
                            next.pending.is_prefetch = false;
                            next.prefetch_dropped = next.prefetch_dropped.wrapping_add(1);
                        } else {
                            next.pending_way = selected_victim;
                            next.refill_beat = 0;
                            next.refill_discard = input.invalidate_all;
                            next.prefetch_armed = false;
                            next.state = State::LineRequest;
                        }
                    }
                }
            }
            State::WordRequest => {
                if input.memory_request_ready {
                    next.state = State::WordResponse;
                }
            }
            State::WordResponse => {
                if input.memory_response_valid {
                    next.response_data = 0;
                    next.response_error = input.memory_error;
                    next.response_valid = true;
                    next.state = State::Idle;
                }
            }
            State::LineRequest => {
                if state.pending.is_prefetch && !state.prefetch_armed {
                    if input.cpu_request_valid || cancel_prefetch {
                        if input.cpu_request_valid && !cancel_prefetch {
                            next.prefetch_dropped = next.prefetch_dropped.wrapping_add(1);
                        }
                        next.pending.is_prefetch = false;
                        next.prefetch_armed = false;
                        next.refill_discard = false;
                        next.state = State::Idle;
                    } else {
                        next.prefetch_armed = true;
                    }
                } else if input.memory_request_ready {
                    next.valid[state.pending_way][set] = false;
                    next.refill_beat = 0;
                    if state.pending.is_prefetch {
                        next.prefetch_issued = next.prefetch_issued.wrapping_add(1);
                    }
                    next.prefetch_armed = false;
                    next.state = State::LineReceive;
                }
            }
            State::LineReceive => {
                if input.memory_response_valid {
                    if input.memory_error {
                        if state.pending.is_prefetch {
                            if !state.refill_discard {
                                next.prefetch_dropped = next.prefetch_dropped.wrapping_add(1);
                            }
                            next.pending.is_prefetch = false;
                        } else {
                            next.response_data = 0;
                            next.response_error = true;
                            next.response_valid = true;
                        }
                        next.state = State::Idle;
                    } else {
                        let first_word = 4 * usize::from(state.refill_beat);
                        let install = !state.refill_discard
                            && !input.invalidate_all
                            && !prefetch_refill_cancelled;
                        if install {
                            for lane in 0..4 {
                                next.data[data_index(state.pending_way, set, first_word + lane)] =
                                    (input.memory_read_data >> (16 * lane)) as u16;
                            }
                        }
                        if first_word <= word && word < first_word + 4 {
                            next.refill_response_data =
                                (input.memory_read_data >> (16 * (word - first_word))) as u16;
                        }
                        if state.refill_beat as usize + 1 == CPU_V3_CACHE_MEMORY_BEATS {
                            if install {
                                if state.pending_way == 1 {
                                    if state.prefetched[1][set] {
                                        next.prefetch_useless =
                                            next.prefetch_useless.wrapping_add(1);
                                    }
                                    next.valid[1][set] = true;
                                    next.prefetched[1][set] = state.pending.is_prefetch;
                                } else {
                                    if state.prefetched[0][set] {
                                        next.prefetch_useless =
                                            next.prefetch_useless.wrapping_add(1);
                                    }
                                    next.valid[0][set] = true;
                                    next.prefetched[0][set] = state.pending.is_prefetch;
                                }
                                next.tags[state.pending_way][set] = tag;
                                next.victim[set] = 1 - state.pending_way;
                            }
                            if !state.pending.is_prefetch {
                                next.response_data = if first_word <= word && word < first_word + 4
                                {
                                    (input.memory_read_data >> (16 * (word - first_word))) as u16
                                } else {
                                    state.refill_response_data
                                };
                                next.response_error = false;
                                next.response_valid = true;
                            }
                            next.pending.is_prefetch = false;
                            next.state = State::Idle;
                        } else {
                            next.refill_beat += 1;
                        }
                    }
                }
            }
        }

        if hit_write {
            next.data[data_index(hit_way, set, word)] = state.pending.write_data;
        }

        if accept_prefetch {
            next.pending = Pending {
                is_prefetch: true,
                write: false,
                address: state.prefetch_pending.unwrap(),
                write_data: 0,
            };
            next.refill_discard = false;
            next.lookup_valid = true;
            next.prefetch_pending = None;
        }

        if accept_cpu_request {
            next.pending = Pending {
                is_prefetch: false,
                write: input.cpu_write,
                address: input.cpu_address as u32,
                write_data: input.cpu_write_data as u16,
            };
            next.response_error = false;
            next.refill_discard = false;
            next.lookup_valid = true;
        }

        if cancel_prefetch {
            let counting = state.prefetch_pending.is_some()
                || (state.pending.is_prefetch
                    && ((state.state == State::Idle && state.lookup_valid)
                        || (state.state == State::LineRequest && !state.prefetch_armed)
                        || ((state.state == State::LineReceive
                            || (state.state == State::LineRequest && state.prefetch_armed))
                            && !state.refill_discard)));
            if counting {
                next.prefetch_dropped = next.prefetch_dropped.wrapping_add(1);
            }
            next.prefetch_pending = None;
            if state.pending.is_prefetch
                && state.state == State::Idle
                && state.lookup_valid
                && !accept_cpu_request
            {
                next.lookup_valid = false;
                next.pending.is_prefetch = false;
            }
            if state.pending.is_prefetch
                && state.state == State::LineRequest
                && !state.prefetch_armed
            {
                next.state = State::Idle;
                next.pending.is_prefetch = false;
                next.prefetch_armed = false;
                next.refill_discard = false;
            } else if state.pending.is_prefetch
                && (state.state == State::LineReceive
                    || (state.state == State::LineRequest && state.prefetch_armed))
            {
                next.refill_discard = true;
            }
        }

        if input.invalidate_all {
            next.prefetch_useless = next.prefetch_useless.wrapping_add(
                state
                    .prefetched
                    .iter()
                    .flatten()
                    .filter(|prefetched| **prefetched)
                    .count() as u32,
            );
            next.valid = [[false; CPU_V3_CACHE_SETS]; CPU_V3_CACHE_WAYS];
            next.prefetched = [[false; CPU_V3_CACHE_SETS]; CPU_V3_CACHE_WAYS];
            if state.state == State::LineRequest || state.state == State::LineReceive {
                next.refill_discard = true;
            }
        }

        *state = next;
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
                    &CpuV3DualPortCacheData::<I>::verilog_identity().module_name(),
                )
                .replace(
                    "__CACHE_TAGS__",
                    &CpuV3CacheTagRam::verilog_identity().module_name(),
                ),
        )
    }

    fn verilog_dependencies() -> Vec<VerilogDependency> {
        vec![
            VerilogDependency::new::<CpuV3DualPortCacheData<I>>("u_data_banks"),
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

#[derive(Clone, ModuleIo)]
pub struct CpuV3DataCacheInput {
    pub reset: Wire,
    pub clean_all: Wire,
    pub invalidate_all: Wire,
    pub cpu_request_valid: Wire,
    pub cpu_write: Wire,
    pub cpu_address: Wires<32>,
    pub cpu_write_data: Wires<16>,
    pub cpu_response_ready: Wire,
    pub memory_request_ready: Wire,
    pub memory_response_valid: Wire,
    pub memory_read_data: Wires<64>,
    pub memory_error: Wire,
}

#[derive(Clone, ModuleIo)]
pub struct CpuV3DataCacheOutput {
    pub cpu_request_ready: Wire,
    pub cpu_response_valid: Wire,
    pub cpu_read_data: Wires<16>,
    pub cpu_error: Wire,
    pub memory_request_valid: Wire,
    pub memory_write: Wire,
    pub memory_line: Wire,
    pub memory_address: Wires<22>,
    pub memory_write_data: Wires<64>,
    pub memory_response_ready: Wire,
    pub maintenance_busy: Wire,
    pub maintenance_done: Wire,
    pub maintenance_error: Wire,
}

pub struct CpuV3DataCache;

#[derive(Clone, ModuleIo)]
struct CpuV3DataCacheDirtyRamInput {
    write_enable: Wire,
    write_way: Wire,
    write_set: Wires<6>,
    write_value: Wire,
    clear_all: Wire,
}

#[derive(Clone, ModuleIo)]
struct CpuV3DataCacheDirtyRamOutput {
    way_0: Wires<64>,
    way_1: Wires<64>,
}

struct CpuV3DataCacheDirtyRam;

impl HardwareIdentity for CpuV3DataCacheDirtyRam {
    const TARGET_RESOURCE_LEAF: bool = true;

    fn verilog_identity() -> VerilogIdentity {
        VerilogIdentity::new("CpuV3DataCacheDirtyRam").namespace(["components", "cpu", "cpu_v3"])
    }
}

impl Module for CpuV3DataCacheDirtyRam {
    type Input = CpuV3DataCacheDirtyRamInput;
    type Output = CpuV3DataCacheDirtyRamOutput;
    type EmuState = ();

    const USES_MAIN_CLOCK: bool = true;
    const EMU_AVAILABLE: bool = false;

    fn target_resources() -> Vec<TargetResourceRequest> {
        vec![TargetResourceRequest::new(SsramBits::new(128))]
    }

    fn execute_emu(
        _state: &mut Self::EmuState,
        _circuit: &mut CircuitWires,
        _input: &Self::Input,
        _output: &Self::Output,
    ) {
        panic!("data-cache dirty SSRAM is Verilog-only")
    }

    fn verilog_source() -> Option<String> {
        Some(include_str!("cpu_v3_data_cache_dirty_ram.v").to_string())
    }

    fn verilog_testbench() -> Option<String> {
        Some(include_str!("cpu_v3_data_cache_dirty_ram_tb.v").to_string())
    }
}

impl HardwareIdentity for CpuV3DataCache {
    const TARGET_RESOURCE_LEAF: bool = false;

    fn verilog_identity() -> VerilogIdentity {
        VerilogIdentity::new("CpuV3DataCache").namespace(["components", "cpu", "cpu_v3"])
    }
}

#[derive(Clone, Copy, Default, Eq, PartialEq)]
enum DataMemoryPhase {
    #[default]
    Idle,
    Lookup,
    WritebackPrime,
    WritebackCapture,
    Request,
    ReadReceive,
    ReadDrain,
    WriteStream,
    WriteResponse,
}

pub struct CpuV3DataCacheState {
    cache: crate::DataCache,
    pending_cpu_request: Option<crate::CpuMemoryRequest>,
    request: Option<crate::MainMemoryRequest>,
    phase: DataMemoryPhase,
    words: [u16; CPU_V3_CACHE_LINE_WORDS],
    beat: usize,
    response_data: u16,
    response_valid: bool,
    response_error: bool,
    maintenance_active: bool,
    maintenance_done: bool,
    maintenance_error: bool,
}

impl Default for CpuV3DataCacheState {
    fn default() -> Self {
        Self {
            cache: crate::DataCache::default(),
            pending_cpu_request: None,
            request: None,
            phase: DataMemoryPhase::Idle,
            words: [0; CPU_V3_CACHE_LINE_WORDS],
            beat: 0,
            response_data: 0,
            response_valid: false,
            response_error: false,
            maintenance_active: false,
            maintenance_done: false,
            maintenance_error: false,
        }
    }
}

impl CpuV3DataCacheState {
    fn start_request(&mut self, request: crate::MainMemoryRequest) {
        let write_line = matches!(request, crate::MainMemoryRequest::WriteLine { .. });
        self.request = Some(request);
        self.phase = if write_line {
            DataMemoryPhase::WritebackPrime
        } else {
            DataMemoryPhase::Request
        };
        self.beat = 0;
    }

    fn apply_action(&mut self, action: crate::CacheAction) {
        match action {
            crate::CacheAction::CpuResponse(response) => {
                self.response_data = match response {
                    crate::CpuMemoryResponse::Read { value } => value,
                    crate::CpuMemoryResponse::WriteComplete => 0,
                };
                self.response_error = false;
                self.response_valid = true;
                self.request = None;
                self.phase = DataMemoryPhase::Idle;
            }
            crate::CacheAction::MainMemoryRequest(request) => self.start_request(request),
        }
    }

    fn fail_transaction(&mut self) {
        // After a physical-memory error no cache line is allowed to remain
        // architecturally visible: the controller may have accepted an
        // unknown prefix of a burst.
        self.cache = crate::DataCache::default();
        self.pending_cpu_request = None;
        self.request = None;
        self.phase = DataMemoryPhase::Idle;
        if self.maintenance_active {
            self.maintenance_active = false;
            self.maintenance_done = true;
            self.maintenance_error = true;
        } else {
            self.response_data = 0;
            self.response_error = true;
            self.response_valid = true;
        }
    }

    fn complete_write(&mut self) {
        if self.maintenance_active {
            match self
                .cache
                .continue_maintenance(crate::MainMemoryResponse::WriteComplete)
                .expect("maintenance completion must match a line write")
            {
                Some(request) => self.start_request(request),
                None => {
                    self.request = None;
                    self.phase = DataMemoryPhase::Idle;
                    self.maintenance_active = false;
                    self.maintenance_done = true;
                }
            }
        } else {
            let action = self
                .cache
                .complete(crate::MainMemoryResponse::WriteComplete)
                .expect("data-cache completion must match a line write");
            self.apply_action(action);
        }
    }
}

impl Module for CpuV3DataCache {
    type Input = CpuV3DataCacheInput;
    type Output = CpuV3DataCacheOutput;
    type EmuState = CpuV3DataCacheState;

    const USES_MAIN_CLOCK: bool = true;

    fn create_emu(_input: &Self::Input, _output: &Self::Output) -> Self::EmuState {
        CpuV3DataCacheState::default()
    }

    fn execute_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        input: &Self::Input,
        output: &Self::Output,
    ) {
        let input = input.sample(circuit);
        let (write, address, write_data) = match state.request {
            Some(crate::MainMemoryRequest::ReadLine { line_address }) => {
                (false, line_address.get(), 0)
            }
            Some(crate::MainMemoryRequest::WriteLine {
                line_address,
                words,
            }) => {
                let index = state.beat.min(CPU_V3_CACHE_MEMORY_BEATS - 1) * 4;
                (
                    true,
                    line_address.get(),
                    u64::from(words[index])
                        | (u64::from(words[index + 1]) << 16)
                        | (u64::from(words[index + 2]) << 32)
                        | (u64::from(words[index + 3]) << 48),
                )
            }
            Some(crate::MainMemoryRequest::WriteWord { .. }) => {
                unreachable!("the write-back cache never emits word writes")
            }
            None => (false, 0, 0),
        };
        output.drive(
            circuit,
            &CpuV3DataCacheOutputValue {
                cpu_request_ready: state.phase == DataMemoryPhase::Idle
                    && !state.response_valid
                    && !state.maintenance_active
                    && !input.clean_all
                    && !input.invalidate_all,
                cpu_response_valid: state.response_valid,
                cpu_read_data: u64::from(state.response_data),
                cpu_error: state.response_valid && state.response_error,
                memory_request_valid: state.phase == DataMemoryPhase::Request,
                memory_write: write,
                memory_line: state.request.is_some(),
                memory_address: u64::from(address),
                memory_write_data: write_data,
                memory_response_ready: matches!(
                    state.phase,
                    DataMemoryPhase::ReadReceive | DataMemoryPhase::WriteResponse
                ),
                maintenance_busy: state.maintenance_active,
                maintenance_done: state.maintenance_done,
                maintenance_error: state.maintenance_error,
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
            *state = CpuV3DataCacheState::default();
            return;
        }
        state.maintenance_done = false;
        if state.response_valid && input.cpu_response_ready {
            state.response_valid = false;
        }

        if state.phase == DataMemoryPhase::Idle
            && !state.response_valid
            && !state.maintenance_active
            && (input.clean_all || input.invalidate_all)
        {
            state.maintenance_active = true;
            state.maintenance_error = false;
            let command = if input.invalidate_all {
                crate::MaintenanceCommand::Invalidate
            } else {
                crate::MaintenanceCommand::Clean
            };
            match state
                .cache
                .begin_maintenance(command)
                .expect("idle data cache must accept maintenance")
            {
                Some(request) => state.start_request(request),
                None => {
                    state.maintenance_active = false;
                    state.maintenance_done = true;
                }
            }
            return;
        }

        if state.phase == DataMemoryPhase::Idle
            && !state.response_valid
            && !state.maintenance_active
            && input.cpu_request_valid
        {
            let address = crate::PhysicalWordAddress::new(input.cpu_address as u32);
            state.pending_cpu_request = Some(if input.cpu_write {
                crate::CpuMemoryRequest::Write {
                    address,
                    value: input.cpu_write_data as u16,
                }
            } else {
                crate::CpuMemoryRequest::Read { address }
            });
            state.phase = DataMemoryPhase::Lookup;
            return;
        }

        if state.phase == DataMemoryPhase::Lookup {
            if let Some(request) = state.pending_cpu_request.take() {
                let address = match request {
                    crate::CpuMemoryRequest::Read { address }
                    | crate::CpuMemoryRequest::Write { address, .. } => address,
                };
                if address.get() >> 22 != 0 {
                    state.response_error = true;
                    state.response_data = 0;
                    state.response_valid = true;
                    state.phase = DataMemoryPhase::Idle;
                } else {
                    let action = state
                        .cache
                        .request(request)
                        .expect("idle data cache must accept a CPU request");
                    state.apply_action(action);
                }
            }
            return;
        }

        match state.phase {
            DataMemoryPhase::Idle => {}
            DataMemoryPhase::Lookup => unreachable!(),
            DataMemoryPhase::WritebackPrime => {
                state.beat = 0;
                state.phase = DataMemoryPhase::WritebackCapture;
            }
            DataMemoryPhase::WritebackCapture => {
                if state.beat == CPU_V3_CACHE_LINE_BEATS - 1 {
                    state.beat = 0;
                    state.phase = DataMemoryPhase::Request;
                } else {
                    state.beat += 1;
                }
            }
            DataMemoryPhase::Request if input.memory_request_ready => {
                state.beat = 0;
                state.phase = if matches!(
                    state.request,
                    Some(crate::MainMemoryRequest::WriteLine { .. })
                ) {
                    state.beat = 1;
                    DataMemoryPhase::WriteStream
                } else {
                    DataMemoryPhase::ReadReceive
                };
            }
            DataMemoryPhase::Request => {}
            DataMemoryPhase::WriteStream => {
                if state.beat == CPU_V3_CACHE_MEMORY_BEATS - 1 {
                    state.phase = DataMemoryPhase::WriteResponse;
                } else {
                    state.beat += 1;
                }
            }
            DataMemoryPhase::WriteResponse if input.memory_response_valid => {
                if input.memory_error {
                    state.fail_transaction();
                } else {
                    state.complete_write();
                }
            }
            DataMemoryPhase::WriteResponse => {}
            DataMemoryPhase::ReadReceive if input.memory_response_valid => {
                if input.memory_error {
                    state.fail_transaction();
                } else {
                    let word = 4 * state.beat;
                    state.words[word] = input.memory_read_data as u16;
                    state.words[word + 1] = (input.memory_read_data >> 16) as u16;
                    state.words[word + 2] = (input.memory_read_data >> 32) as u16;
                    state.words[word + 3] = (input.memory_read_data >> 48) as u16;
                    if state.beat == CPU_V3_CACHE_MEMORY_BEATS - 1 {
                        state.beat = 0;
                        state.phase = DataMemoryPhase::ReadDrain;
                    } else {
                        state.beat += 1;
                    }
                }
            }
            DataMemoryPhase::ReadReceive => {}
            DataMemoryPhase::ReadDrain => {
                if state.beat == CPU_V3_CACHE_LINE_BEATS - 1 {
                    let action = state
                        .cache
                        .complete(crate::MainMemoryResponse::ReadLine { words: state.words })
                        .expect("data-cache completion must match a line read");
                    state.apply_action(action);
                } else {
                    state.beat += 1;
                }
            }
        }
    }

    fn verilog_source() -> Option<String> {
        Some(
            include_str!("cpu_v3_data_cache.v")
                .replace(
                    "__CACHE_DATA_BANKS__",
                    &CpuV3DualPortCacheData::<ZeroBsramImage>::verilog_identity().module_name(),
                )
                .replace(
                    "__CACHE_TAGS__",
                    &CpuV3CacheTagRam::verilog_identity().module_name(),
                )
                .replace(
                    "__DIRTY_RAM__",
                    &CpuV3DataCacheDirtyRam::verilog_identity().module_name(),
                ),
        )
    }

    fn verilog_dependencies() -> Vec<VerilogDependency> {
        vec![
            VerilogDependency::new::<CpuV3DualPortCacheData<ZeroBsramImage>>("u_data_banks"),
            VerilogDependency::new::<CpuV3CacheTagRam>("u_tags"),
            VerilogDependency::new::<CpuV3DataCacheDirtyRam>("u_dirty"),
        ]
    }

    fn verilog_testbench() -> Option<String> {
        Some(include_str!("cpu_v3_data_cache_tb.v").to_string())
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
    /// returns four ordered 64-bit beats, a write
    /// request returns one completion beat.
    struct MemoryModel {
        words: HashMap<u32, u16>,
        beats: VecDeque<(u64, bool)>,
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
                for beat in 0..CPU_V3_CACHE_MEMORY_BEATS as u32 {
                    let word_0 = self.word(address + 4 * beat);
                    let word_1 = self.word(address + 4 * beat + 1);
                    let word_2 = self.word(address + 4 * beat + 2);
                    let word_3 = self.word(address + 4 * beat + 3);
                    let error = self.error_on_beat == Some(beat as usize);
                    self.beats.push_back((
                        u64::from(word_0)
                            | u64::from(word_1) << 16
                            | u64::from(word_2) << 32
                            | u64::from(word_3) << 48,
                        error,
                    ));
                }
            }
        }

        fn word(&self, address: u32) -> u16 {
            self.words.get(&address).copied().unwrap_or(0)
        }
    }

    fn drive(
        circuit: &mut Circuit,
        input: &CpuV3TwoWayCacheInput,
        cpu_request: Option<(bool, u32, u16)>,
        cpu_response_ready: bool,
        memory_response: Option<(u64, bool)>,
        invalidate_all: bool,
    ) {
        let (cpu_write, cpu_address, cpu_write_data) = cpu_request.unwrap_or_default();
        input.drive(
            circuit,
            &CpuV3TwoWayCacheInputValue {
                reset: false,
                invalidate_all,
                prefetch_request_valid: false,
                prefetch_address: 0,
                prefetch_cancel: false,
                cpu_request_valid: cpu_request.is_some(),
                cpu_write,
                cpu_address: u64::from(cpu_address),
                cpu_write_data: u64::from(cpu_write_data),
                cpu_response_ready,
                memory_request_ready: true,
                memory_response_valid: memory_response.is_some(),
                memory_read_data: memory_response.unwrap_or_default().0,
                memory_error: memory_response.unwrap_or_default().1,
            },
        );
    }

    fn transact(
        circuit: &mut Circuit,
        input: &CpuV3TwoWayCacheInput,
        output: &CpuV3TwoWayCacheOutput,
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

    fn fixture() -> (Circuit, CpuV3TwoWayCacheInput, CpuV3TwoWayCacheOutput) {
        let (circuit, (input, output)) = build_circuit(|| {
            let input = CpuV3TwoWayCacheInput::allocate();
            let output = CpuV3TwoWayCache::emu(&input);
            (input, output)
        });
        (circuit, input, output)
    }

    #[test]
    fn cache_image_is_split_by_parity_and_initializes_only_way_zero() {
        for address in 0..CPU_V3_CACHE_WORDS_PER_WAY / 2 {
            assert_eq!(
                parity_bank_image::<InterleavedImage, false>()[address],
                (2 * address) as u64
            );
            assert_eq!(
                parity_bank_image::<InterleavedImage, true>()[address],
                (2 * address + 1) as u64
            );
        }
        assert!(
            parity_bank_image::<InterleavedImage, false>()[CPU_V3_CACHE_WORDS_PER_WAY / 2..]
                .iter()
                .all(|word| *word == 0)
        );
        assert!(
            parity_bank_image::<InterleavedImage, true>()[CPU_V3_CACHE_WORDS_PER_WAY / 2..]
                .iter()
                .all(|word| *word == 0)
        );
    }

    #[test]
    fn miss_refills_one_line_through_four_beats_and_hits_afterwards() {
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
        let project = VerilogProject::generate::<CpuV3TwoWayCache>().unwrap();
        let resources: Vec<_> = project
            .resource_claims
            .iter()
            .flat_map(|claim| claim.resources.iter().copied())
            .collect();
        assert!(resources.contains(&ResourceAmount::new(ResourceKind::Bsram18K, 2)));
        assert!(resources.contains(&ResourceAmount::new(
            ResourceKind::SsramBit,
            CPU_V3_CACHE_TAG_PHYSICAL_BITS as u64,
        )));
    }

    #[test]
    #[ignore = "explicit external simulation of the CpuV3 two-way cache"]
    fn verify_verilog_with_iverilog() {
        digital_design_hardware::verify_verilog_with_iverilog::<CpuV3TwoWayCache>().unwrap();
    }

    #[test]
    fn data_cache_exports_two_data_bsrams_tag_ssram_and_dirty_ssram() {
        let project = VerilogProject::generate::<CpuV3DataCache>().unwrap();
        let resources: Vec<_> = project
            .resource_claims
            .iter()
            .flat_map(|claim| claim.resources.iter().copied())
            .collect();
        assert!(resources.contains(&ResourceAmount::new(ResourceKind::Bsram18K, 2)));
        assert!(resources.contains(&ResourceAmount::new(
            ResourceKind::SsramBit,
            CPU_V3_CACHE_TAG_PHYSICAL_BITS as u64,
        )));
        assert!(resources.contains(&ResourceAmount::new(ResourceKind::SsramBit, 128)));
    }

    #[test]
    #[ignore = "explicit external simulation of the write-back data cache"]
    fn verify_data_cache_with_iverilog() {
        digital_design_hardware::verify_verilog_with_iverilog::<CpuV3DataCache>().unwrap();
    }

    #[test]
    #[ignore = "explicit external simulation of the read-only instruction cache"]
    fn verify_instruction_cache_with_iverilog() {
        digital_design_hardware::verify_verilog_with_iverilog::<CpuV3InstructionCache>().unwrap();
    }

    // ---- emulator vs RTL co-simulation ----

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    struct CycleOut {
        cpu_request_ready: bool,
        cpu_response_valid: bool,
        cpu_read_data: u16,
        cpu_error: bool,
        memory_request_valid: bool,
        memory_write: bool,
        memory_address: u32,
        memory_response_ready: bool,
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    struct CycleIn {
        invalidate_all: bool,
        prefetch_request_valid: bool,
        prefetch_address: u32,
        prefetch_cancel: bool,
        cpu_request_valid: bool,
        cpu_write: bool,
        cpu_address: u32,
        cpu_write_data: u16,
        cpu_response_ready: bool,
        memory_response_valid: bool,
        memory_read_data: u64,
        memory_error: bool,
    }

    #[allow(clippy::too_many_arguments)]
    fn cosim_step(
        circuit: &mut Circuit,
        input: &CpuV3TwoWayCacheInput,
        output: &CpuV3TwoWayCacheOutput,
        memory: &mut MemoryModel,
        cpu_request: Option<(bool, u32, u16)>,
        cpu_response_ready: bool,
        prefetch: Option<u32>,
        prefetch_cancel: bool,
        invalidate_all: bool,
        trace: &mut Vec<(CycleIn, CycleOut)>,
    ) -> CycleOut {
        let memory_response = memory.beats.pop_front();
        let (cpu_write, cpu_address, cpu_write_data) = cpu_request.unwrap_or((false, 0, 0));
        input.drive(
            circuit,
            &CpuV3TwoWayCacheInputValue {
                reset: false,
                invalidate_all,
                prefetch_request_valid: prefetch.is_some(),
                prefetch_address: u64::from(prefetch.unwrap_or(0)),
                prefetch_cancel,
                cpu_request_valid: cpu_request.is_some(),
                cpu_write,
                cpu_address: u64::from(cpu_address),
                cpu_write_data: u64::from(cpu_write_data),
                cpu_response_ready,
                memory_request_ready: true,
                memory_response_valid: memory_response.is_some(),
                memory_read_data: memory_response.unwrap_or_default().0,
                memory_error: memory_response.unwrap_or_default().1,
            },
        );
        circuit.execute_gates();
        let value = output.sample(circuit);
        if value.memory_request_valid {
            memory.accept(
                value.memory_write,
                value.memory_address as u32,
                value.memory_write_data as u16,
            );
        }
        let out = CycleOut {
            cpu_request_ready: value.cpu_request_ready,
            cpu_response_valid: value.cpu_response_valid,
            cpu_read_data: value.cpu_read_data as u16,
            cpu_error: value.cpu_error,
            memory_request_valid: value.memory_request_valid,
            memory_write: value.memory_write,
            memory_address: value.memory_address as u32,
            memory_response_ready: value.memory_response_ready,
        };
        let cin = CycleIn {
            invalidate_all,
            prefetch_request_valid: prefetch.is_some(),
            prefetch_address: prefetch.unwrap_or(0),
            prefetch_cancel,
            cpu_request_valid: cpu_request.is_some(),
            cpu_write,
            cpu_address,
            cpu_write_data,
            cpu_response_ready,
            memory_response_valid: memory_response.is_some(),
            memory_read_data: memory_response.unwrap_or_default().0,
            memory_error: memory_response.unwrap_or_default().1,
        };
        trace.push((cin, out));
        circuit.clock_tick();
        out
    }

    fn cosim_read(
        circuit: &mut Circuit,
        input: &CpuV3TwoWayCacheInput,
        output: &CpuV3TwoWayCacheOutput,
        memory: &mut MemoryModel,
        address: u32,
        trace: &mut Vec<(CycleIn, CycleOut)>,
    ) -> u16 {
        let mut request = Some((false, address, 0));
        while request.is_some() {
            let out = cosim_step(
                circuit, input, output, memory, request, false, None, false, false, trace,
            );
            if out.cpu_request_ready {
                request = None;
            }
        }
        loop {
            let out = cosim_step(
                circuit, input, output, memory, None, false, None, false, false, trace,
            );
            if out.cpu_response_valid {
                let data = out.cpu_read_data;
                cosim_step(
                    circuit, input, output, memory, None, true, None, false, false, trace,
                );
                return data;
            }
        }
    }

    fn cosim_write(
        circuit: &mut Circuit,
        input: &CpuV3TwoWayCacheInput,
        output: &CpuV3TwoWayCacheOutput,
        memory: &mut MemoryModel,
        address: u32,
        write_data: u16,
        trace: &mut Vec<(CycleIn, CycleOut)>,
    ) {
        let mut request = Some((true, address, write_data));
        while request.is_some() {
            let out = cosim_step(
                circuit, input, output, memory, request, false, None, false, false, trace,
            );
            if out.cpu_request_ready {
                request = None;
            }
        }
        loop {
            let out = cosim_step(
                circuit, input, output, memory, None, false, None, false, false, trace,
            );
            if out.cpu_response_valid {
                cosim_step(
                    circuit, input, output, memory, None, true, None, false, false, trace,
                );
                return;
            }
        }
    }

    fn run_emu_trace() -> Vec<(CycleIn, CycleOut)> {
        let (mut circuit, input, output) = fixture();
        let mut memory = MemoryModel::new();
        for address in 0x120..0x130 {
            memory
                .words
                .insert(address, (0x8000 | (address & 0xff)) as u16);
        }
        for address in 0x1520..0x1530 {
            memory
                .words
                .insert(address, (0x9000 | (address & 0xff)) as u16);
        }
        let mut trace = Vec::new();

        // Cold miss refills line 0x120, then a hit.
        assert_eq!(
            cosim_read(
                &mut circuit,
                &input,
                &output,
                &mut memory,
                0x123,
                &mut trace
            ),
            0x8023
        );
        assert_eq!(
            cosim_read(
                &mut circuit,
                &input,
                &output,
                &mut memory,
                0x124,
                &mut trace
            ),
            0x8024
        );

        // Write-through, then a hit returns the written value.
        cosim_write(
            &mut circuit,
            &input,
            &output,
            &mut memory,
            0x123,
            0x4567,
            &mut trace,
        );
        assert_eq!(
            cosim_read(
                &mut circuit,
                &input,
                &output,
                &mut memory,
                0x123,
                &mut trace
            ),
            0x4567
        );

        // Back-to-back reads exercise the pipelined hit path: the second
        // request is accepted in the same cycle the first lookup resolves.
        cosim_step(
            &mut circuit,
            &input,
            &output,
            &mut memory,
            Some((false, 0x123, 0)),
            false,
            None,
            false,
            false,
            &mut trace,
        );
        cosim_step(
            &mut circuit,
            &input,
            &output,
            &mut memory,
            Some((false, 0x124, 0)),
            false,
            None,
            false,
            false,
            &mut trace,
        );
        for _ in 0..8 {
            let out = cosim_step(
                &mut circuit,
                &input,
                &output,
                &mut memory,
                None,
                false,
                None,
                false,
                false,
                &mut trace,
            );
            if out.cpu_response_valid {
                cosim_step(
                    &mut circuit,
                    &input,
                    &output,
                    &mut memory,
                    None,
                    true,
                    None,
                    false,
                    false,
                    &mut trace,
                );
            }
        }

        // Nominate a next-line prefetch; idle cycles let it issue and drain.
        cosim_step(
            &mut circuit,
            &input,
            &output,
            &mut memory,
            None,
            false,
            Some(0x1520),
            false,
            false,
            &mut trace,
        );
        for _ in 0..40 {
            cosim_step(
                &mut circuit,
                &input,
                &output,
                &mut memory,
                None,
                false,
                None,
                false,
                false,
                &mut trace,
            );
        }
        // The prefetched line is present, so the demand read hits it.
        assert_eq!(
            cosim_read(
                &mut circuit,
                &input,
                &output,
                &mut memory,
                0x1523,
                &mut trace
            ),
            0x9023
        );

        // A redirect may cancel a prefetch lookup in the same cycle that a
        // demand replaces it. The accepted demand must remain live.
        for address in 0x2520..0x2530 {
            memory
                .words
                .insert(address, (0xa000 | (address & 0xff)) as u16);
        }
        cosim_step(
            &mut circuit,
            &input,
            &output,
            &mut memory,
            None,
            false,
            Some(0x2120),
            false,
            false,
            &mut trace,
        );
        cosim_step(
            &mut circuit,
            &input,
            &output,
            &mut memory,
            None,
            false,
            None,
            false,
            false,
            &mut trace,
        );
        let out = cosim_step(
            &mut circuit,
            &input,
            &output,
            &mut memory,
            Some((false, 0x2523, 0)),
            false,
            None,
            true,
            false,
            &mut trace,
        );
        assert!(
            out.cpu_request_ready,
            "cancel must not reject the replacing demand"
        );
        let mut replaced_data = None;
        for _ in 0..40 {
            let out = cosim_step(
                &mut circuit,
                &input,
                &output,
                &mut memory,
                None,
                false,
                None,
                false,
                false,
                &mut trace,
            );
            if out.cpu_response_valid {
                replaced_data = Some(out.cpu_read_data);
                cosim_step(
                    &mut circuit,
                    &input,
                    &output,
                    &mut memory,
                    None,
                    true,
                    None,
                    false,
                    false,
                    &mut trace,
                );
                break;
            }
        }
        assert_eq!(replaced_data, Some(0xa023));

        // A prefetch cancellation during a demand refill drain must not turn
        // the completed demand line into a subsequent miss.
        for address in 0x2920..0x2930 {
            memory
                .words
                .insert(address, (0xb000 | (address & 0xff)) as u16);
        }
        let requests_before = memory.requests;
        let out = cosim_step(
            &mut circuit,
            &input,
            &output,
            &mut memory,
            Some((false, 0x2923, 0)),
            false,
            None,
            false,
            false,
            &mut trace,
        );
        assert!(out.cpu_request_ready);
        cosim_step(
            &mut circuit,
            &input,
            &output,
            &mut memory,
            None,
            false,
            None,
            false,
            false,
            &mut trace,
        );
        cosim_step(
            &mut circuit,
            &input,
            &output,
            &mut memory,
            None,
            false,
            None,
            false,
            false,
            &mut trace,
        );
        for _ in 0..8 {
            cosim_step(
                &mut circuit,
                &input,
                &output,
                &mut memory,
                None,
                false,
                None,
                false,
                false,
                &mut trace,
            );
        }
        cosim_step(
            &mut circuit,
            &input,
            &output,
            &mut memory,
            None,
            false,
            None,
            true,
            false,
            &mut trace,
        );
        let mut demand_data = None;
        for _ in 0..16 {
            let out = cosim_step(
                &mut circuit,
                &input,
                &output,
                &mut memory,
                None,
                false,
                None,
                false,
                false,
                &mut trace,
            );
            if out.cpu_response_valid {
                demand_data = Some(out.cpu_read_data);
                cosim_step(
                    &mut circuit,
                    &input,
                    &output,
                    &mut memory,
                    None,
                    true,
                    None,
                    false,
                    false,
                    &mut trace,
                );
                break;
            }
        }
        assert_eq!(demand_data, Some(0xb023));
        assert_eq!(
            cosim_read(
                &mut circuit,
                &input,
                &output,
                &mut memory,
                0x2924,
                &mut trace
            ),
            0xb024
        );
        assert_eq!(
            memory.requests,
            requests_before + 1,
            "demand line was not installed while canceling prefetch"
        );

        // Invalidate clears the cache; the next read misses again.
        cosim_step(
            &mut circuit,
            &input,
            &output,
            &mut memory,
            None,
            false,
            None,
            false,
            true,
            &mut trace,
        );
        cosim_step(
            &mut circuit,
            &input,
            &output,
            &mut memory,
            None,
            false,
            None,
            false,
            false,
            &mut trace,
        );
        assert_eq!(
            cosim_read(
                &mut circuit,
                &input,
                &output,
                &mut memory,
                0x123,
                &mut trace
            ),
            0x4567
        );

        trace
    }

    fn generate_cosim_tb(trace: &[(CycleIn, CycleOut)], module_name: &str) -> String {
        let mut t = format!(
            "module tb;\n\
             reg clk = 0;\n\
             reg reset, invalidate_all, prefetch_request_valid, prefetch_cancel;\n\
             reg [31:0] prefetch_address;\n\
             reg cpu_request_valid, cpu_write, cpu_response_ready;\n\
             reg [31:0] cpu_address;\n\
             reg [15:0] cpu_write_data;\n\
             reg memory_request_ready, memory_response_valid, memory_error;\n\
             reg [63:0] memory_read_data;\n\
             wire cpu_request_ready, cpu_response_valid, cpu_error;\n\
             wire [15:0] cpu_read_data;\n\
             wire memory_request_valid, memory_write, memory_line, memory_response_ready;\n\
             wire [21:0] memory_address;\n\
             wire [63:0] memory_write_data;\n\
             wire [31:0] prefetch_issued, prefetch_useful, prefetch_useless, prefetch_dropped;\n\n\
             {module_name} dut(.*);\n\n\
             always #5 clk = ~clk;\n\n\
             initial begin\n\
                 reset = 1; invalidate_all = 0; prefetch_request_valid = 0; prefetch_address = 0;\n\
                 prefetch_cancel = 0; cpu_request_valid = 0; cpu_write = 0; cpu_address = 0;\n\
                 cpu_write_data = 0; cpu_response_ready = 0; memory_request_ready = 1;\n\
                 memory_response_valid = 0; memory_read_data = 0; memory_error = 0;\n\
                 repeat (2) @(posedge clk);\n\
                 reset = 0;\n\
                 @(posedge clk);\n\
                 @(negedge clk);\n",
        );
        for (i, (cin, _)) in trace.iter().enumerate() {
            t.push_str(&format!(
                "    // cycle {i}\n\
                 cpu_request_valid = 1'b{crv}; cpu_write = 1'b{cw}; cpu_address = 32'h{ca:08x}; cpu_write_data = 16'h{cwd:04x};\n\
                 cpu_response_ready = 1'b{crr}; invalidate_all = 1'b{inv}; prefetch_request_valid = 1'b{prv}; prefetch_address = 32'h{pa:08x}; prefetch_cancel = 1'b{pc};\n\
                 memory_response_valid = 1'b{mrv}; memory_read_data = 64'h{mrd:016x}; memory_error = 1'b{me};\n\
                 #1;\n\
                 $display(\"OUT %0d %0d %0d %0d %0d %0d %0d %0d %0d\", {i}, cpu_request_ready, cpu_response_valid, cpu_read_data, cpu_error, memory_request_valid, memory_write, memory_address, memory_response_ready);\n\
                 @(posedge clk);\n\
                 @(negedge clk);\n",
                crv = u8::from(cin.cpu_request_valid),
                cw = u8::from(cin.cpu_write),
                ca = cin.cpu_address,
                cwd = cin.cpu_write_data,
                crr = u8::from(cin.cpu_response_ready),
                inv = u8::from(cin.invalidate_all),
                prv = u8::from(cin.prefetch_request_valid),
                pa = cin.prefetch_address,
                pc = u8::from(cin.prefetch_cancel),
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

    fn cache_cosim_sources() -> String {
        let mut s = String::new();
        s.push_str(&CpuV3DualPortCacheData::<ZeroBsramImage>::verilog_source().unwrap());
        s.push('\n');
        s.push_str(&CpuV3CacheTagRam::verilog_source().unwrap());
        s.push('\n');
        s.push_str(&CpuV3TwoWayCache::verilog_source().unwrap());
        s
    }

    fn run_iverilog_cosim(sources: &str, tb: &str) -> Vec<CycleOut> {
        let directory = std::env::temp_dir().join(format!("cache-cosim-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("modules.v"), sources).unwrap();
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
                assert_eq!(fields.len(), 9, "unexpected OUT line: {line}");
                outputs.push(CycleOut {
                    cpu_request_ready: fields[1] == "1",
                    cpu_response_valid: fields[2] == "1",
                    cpu_read_data: fields[3].parse().unwrap(),
                    cpu_error: fields[4] == "1",
                    memory_request_valid: fields[5] == "1",
                    memory_write: fields[6] == "1",
                    memory_address: fields[7].parse().unwrap(),
                    memory_response_ready: fields[8] == "1",
                });
            } else if line == "TRACE_END" {
                break;
            }
        }
        std::fs::remove_dir_all(&directory).ok();
        outputs
    }

    #[test]
    #[ignore = "explicit emulator-vs-Icarus co-simulation of the two-way cache"]
    fn emu_matches_rtl_verilog() {
        let trace = run_emu_trace();
        let module_name = CpuV3TwoWayCache::verilog_identity().module_name();
        let tb = generate_cosim_tb(&trace, &module_name);
        let rtl = run_iverilog_cosim(&cache_cosim_sources(), &tb);
        assert_eq!(rtl.len(), trace.len(), "cycle count mismatch");
        for (i, ((_, expected), actual)) in trace.iter().zip(&rtl).enumerate() {
            assert_eq!(*actual, *expected, "emu/RTL output mismatch at cycle {i}");
        }
    }
}
