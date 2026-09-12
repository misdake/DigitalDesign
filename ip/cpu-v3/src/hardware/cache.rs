//! CpuV3 two-way physical-address cache hardware.
//!
//! Cache lines use four ordered 64-bit memory beats. Two true-dual-port data
//! BSRAMs split each line by word parity and directly transfer all four words
//! of a beat. Tags and valid bits commit only after a complete error-free line. The
//! production I-cache exposes a read-only boundary around this refill
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

/// The valid/victim leaf holds three 64-deep one-bit arrays: valid way zero,
/// valid way one, and the victim bit. Gowin builds each of them from four
/// RAM16 cells, so the leaf claims twelve cells. The claim uses the same
/// "physical bits" convention as the tag array (the resource audit charges
/// every SSRAM cell as 64 bits); in real RAM16S1/RAM16SDP1 cells the leaf is
/// 12 x 16 bits. The inference only happens while no array write takes its way
/// or enable from that array's own asynchronous read data, which is why both
/// caches clear the victim from the registered pending way instead of from the
/// combinationally selected victim or the request handshake.
const CPU_V3_CACHE_VALID_RAM16S: usize = 3 * CPU_V3_CACHE_SETS.div_ceil(16);
const CPU_V3_CACHE_VALID_PHYSICAL_BITS: usize = CPU_V3_CACHE_VALID_RAM16S * 64;

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

/// RAM16 valid (2 x 64) and victim (64) arrays with asynchronous read, a
/// synchronous single-way write port, and a sweep clear that takes priority
/// and clears both ways of one set per cycle. Way zero initializes from the
/// cache image's INITIAL_VALID mask; all other bits start cleared.
///
/// Every array write must select a registered way: a way select derived from
/// this leaf's own read data closes a combinational loop through the array and
/// makes Gowin map all three arrays as flip-flops plus read multiplexers
/// instead of RAM16 cells.
pub struct CpuV3CacheValidRamWithImage<I>(PhantomData<I>);
pub type CpuV3CacheValidRam = CpuV3CacheValidRamWithImage<ZeroBsramImage>;

impl<I: CpuV3CacheImage> HardwareIdentity for CpuV3CacheValidRamWithImage<I> {
    const TARGET_RESOURCE_LEAF: bool = true;

    fn verilog_identity() -> VerilogIdentity {
        VerilogIdentity::new("CpuV3CacheValidRam")
            .namespace(["components", "cpu", "cpu_v3"])
            .symbol("IMAGE", format!("v{:016x}", I::INITIAL_VALID))
    }
}

#[derive(Clone, ModuleIo)]
pub struct CpuV3CacheValidRamInput {
    pub clear_enable: Wire,
    pub clear_set: Wires<6>,
    pub write_enable: Wire,
    pub write_way: Wire,
    pub write_set: Wires<6>,
    pub write_value: Wire,
    pub victim_write_enable: Wire,
    pub victim_write_value: Wire,
    pub read_set: Wires<6>,
}

#[derive(Clone, ModuleIo)]
pub struct CpuV3CacheValidRamOutput {
    pub way_0_valid: Wire,
    pub way_1_valid: Wire,
    pub victim: Wire,
}

impl<I: CpuV3CacheImage> Module for CpuV3CacheValidRamWithImage<I> {
    type Input = CpuV3CacheValidRamInput;
    type Output = CpuV3CacheValidRamOutput;
    type EmuState = ();

    const USES_MAIN_CLOCK: bool = true;
    const EMU_AVAILABLE: bool = false;

    fn target_resources() -> Vec<TargetResourceRequest> {
        vec![TargetResourceRequest::new(SsramBits::new(
            CPU_V3_CACHE_VALID_PHYSICAL_BITS as u64,
        ))]
    }

    fn execute_emu(
        _state: &mut Self::EmuState,
        _circuit: &mut CircuitWires,
        _input: &Self::Input,
        _output: &Self::Output,
    ) {
        panic!("valid/victim RAM16 is Verilog-only")
    }

    fn verilog_source() -> Option<String> {
        let module_name = Self::verilog_identity().module_name();
        Some(format!(
            r#"module {module_name} (
    input wire clk,
    input wire clear_enable,
    input wire [5:0] clear_set,
    input wire write_enable,
    input wire write_way,
    input wire [5:0] write_set,
    input wire write_value,
    input wire victim_write_enable,
    input wire victim_write_value,
    input wire [5:0] read_set,
    output wire way_0_valid,
    output wire way_1_valid,
    output wire victim
);
reg way_0_valid_ram [0:63];
reg way_1_valid_ram [0:63];
reg victim_ram [0:63];
localparam [63:0] INITIAL_VALID = 64'h{:016x};
integer initial_set;
initial begin
    for (initial_set = 0; initial_set < 64; initial_set = initial_set + 1) begin
        way_0_valid_ram[initial_set] = INITIAL_VALID[initial_set];
        way_1_valid_ram[initial_set] = 1'b0;
        victim_ram[initial_set] = 1'b0;
    end
end
always @(posedge clk) begin
    if (clear_enable) begin
        way_0_valid_ram[clear_set] <= 1'b0;
        way_1_valid_ram[clear_set] <= 1'b0;
    end else if (write_enable) begin
        if (write_way) way_1_valid_ram[write_set] <= write_value;
        else way_0_valid_ram[write_set] <= write_value;
    end
    if (victim_write_enable)
        victim_ram[write_set] <= victim_write_value;
end
assign way_0_valid = way_0_valid_ram[read_set];
assign way_1_valid = way_1_valid_ram[read_set];
assign victim = victim_ram[read_set];
endmodule
"#,
            I::INITIAL_VALID
        ))
    }

    fn verilog_testbench() -> Option<String> {
        Some(include_str!("cpu_v3_cache_valid_ram_tb.v").replace(
            "CpuV3CacheValidRam dut",
            &format!("{} dut", Self::verilog_identity().module_name()),
        ))
    }
}

#[derive(Clone, ModuleIo)]
pub struct CpuV3TwoWayCacheInput {
    pub reset: Wire,
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
}

pub struct CpuV3TwoWayCacheWithImage<I>(PhantomData<I>);
pub type CpuV3TwoWayCache = CpuV3TwoWayCacheWithImage<ZeroBsramImage>;

#[derive(Clone, ModuleIo)]
pub struct CpuV3InstructionCacheInput {
    pub reset: Wire,
    pub invalidate_all: Wire,
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
}

/// Production read-only I-cache boundary. The proven refill engine is
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
    write: bool,
    address: u32,
    write_data: u16,
}

#[derive(Clone)]
pub struct CpuV3TwoWayCacheState {
    data: Box<[u16; CPU_V3_CACHE_WORDS]>,
    tags: [[u16; CPU_V3_CACHE_SETS]; CPU_V3_CACHE_WAYS],
    valid: [[bool; CPU_V3_CACHE_SETS]; CPU_V3_CACHE_WAYS],
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
    refill_discard: bool,
    // Mirror of the RTL RAM16 valid/victim arrays: `valid`/`victim` hold the
    // RAM contents and `invalidate_all` starts a 64-set sweep (both ways
    // cleared in parallel, one set per cycle) instead of an instant clear.
    // New requests are blocked through cpu_request_ready while it runs.
    sweep_active: bool,
    sweep_set: u8,
}

impl Default for CpuV3TwoWayCacheState {
    fn default() -> Self {
        Self {
            data: Box::new([0; CPU_V3_CACHE_WORDS]),
            tags: [[0; CPU_V3_CACHE_SETS]; CPU_V3_CACHE_WAYS],
            valid: [[false; CPU_V3_CACHE_SETS]; CPU_V3_CACHE_WAYS],
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
            refill_discard: false,
            sweep_active: false,
            sweep_set: 0,
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
        let invalidating = input.invalidate_all || state.sweep_active;
        let way_0_hit = state.valid[0][set] && state.tags[0][set] == tag;
        let way_1_hit = state.valid[1][set] && state.tags[1][set] == tag;
        let pending_hit = way_0_hit || way_1_hit;
        let lookup_read_hit =
            state.lookup_valid && pending_address_valid && !state.pending.write && pending_hit;
        let response_space = !state.response_valid || input.cpu_response_ready;
        let cpu_request_ready = !invalidating
            && state.state == State::Idle
            && (!state.lookup_valid || lookup_read_hit && response_space);
        output.drive(
            circuit,
            &CpuV3TwoWayCacheOutputValue {
                cpu_request_ready,
                cpu_response_valid: state.response_valid,
                cpu_read_data: u64::from(state.response_data),
                cpu_error: state.response_valid && state.response_error,
                memory_request_valid: state.state == State::WordRequest
                    || state.state == State::LineRequest,
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
        let invalidating = input.invalidate_all || state.sweep_active;
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
            && !invalidating;
        let lookup_read_hit =
            state.lookup_valid && pending_address_valid && !state.pending.write && pending_hit;
        let cpu_request_ready = !invalidating
            && state.state == State::Idle
            && (!state.lookup_valid || lookup_read_hit && response_space);
        let accept_cpu_request = input.cpu_request_valid && cpu_request_ready;

        let mut next = state.clone();

        if state.response_valid && input.cpu_response_ready {
            next.response_valid = false;
        }

        // Sweep control: invalidate_all (re)starts the 64-set sweep; each
        // following edge clears both ways of one set. The RAM write port
        // gives the sweep priority over any line-request/commit write, which
        // is therefore dropped while a sweep runs.
        if input.invalidate_all {
            next.sweep_active = true;
            next.sweep_set = 0;
        } else if state.sweep_active {
            next.valid[0][state.sweep_set as usize] = false;
            next.valid[1][state.sweep_set as usize] = false;
            next.sweep_set = state.sweep_set + 1;
            if state.sweep_set == 63 {
                next.sweep_active = false;
            }
        }

        match state.state {
            State::Idle => {
                if state.lookup_valid && response_space {
                    if !pending_address_valid {
                        next.response_data = 0;
                        next.response_error = true;
                        next.response_valid = true;
                        next.lookup_valid = false;
                    } else if state.pending.write {
                        next.lookup_valid = false;
                        next.state = State::WordRequest;
                    } else if pending_hit {
                        next.response_data = state.data[data_index(hit_way, set, word)];
                        next.response_error = false;
                        next.response_valid = true;
                        next.lookup_valid = false;
                    } else {
                        next.lookup_valid = false;
                        next.pending_way = selected_victim;
                        next.refill_beat = 0;
                        // An invalidate coincident with miss detection belongs
                        // to the old fetch epoch. Complete its protocol
                        // response, but never install the line.
                        next.refill_discard = input.invalidate_all;
                        next.state = State::LineRequest;
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
                if input.memory_request_ready {
                    if !state.sweep_active {
                        next.valid[state.pending_way][set] = false;
                    }
                    next.refill_beat = 0;
                    next.state = State::LineReceive;
                }
            }
            State::LineReceive => {
                if input.memory_response_valid {
                    if input.memory_error {
                        next.response_data = 0;
                        next.response_error = true;
                        next.response_valid = true;
                        next.state = State::Idle;
                    } else {
                        let first_word = 4 * usize::from(state.refill_beat);
                        let install = !state.refill_discard && !invalidating;
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
                                    next.valid[1][set] = true;
                                } else {
                                    next.valid[0][set] = true;
                                }
                                next.tags[state.pending_way][set] = tag;
                                next.victim[set] = 1 - state.pending_way;
                            }
                            next.response_data = if first_word <= word && word < first_word + 4 {
                                (input.memory_read_data >> (16 * (word - first_word))) as u16
                            } else {
                                state.refill_response_data
                            };
                            next.response_error = false;
                            next.response_valid = true;
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

        if accept_cpu_request {
            next.pending = Pending {
                write: input.cpu_write,
                address: input.cpu_address as u32,
                write_data: input.cpu_write_data as u16,
            };
            next.response_error = false;
            next.refill_discard = false;
            next.lookup_valid = true;
        }

        if input.invalidate_all
            && (state.state == State::LineRequest || state.state == State::LineReceive)
        {
            next.refill_discard = true;
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
                )
                .replace(
                    "__CACHE_VALID__",
                    &CpuV3CacheValidRamWithImage::<I>::verilog_identity().module_name(),
                ),
        )
    }

    fn verilog_dependencies() -> Vec<VerilogDependency> {
        vec![
            VerilogDependency::new::<CpuV3DualPortCacheData<I>>("u_data_banks"),
            VerilogDependency::new::<CpuV3CacheTagRam>("u_tags"),
            VerilogDependency::new::<CpuV3CacheValidRamWithImage<I>>("u_valid"),
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
    pub valid_sweep: Wire,
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
        // The maintenance scan reads both whole 64-bit words every cycle
        // (`assign way_0 = dirty[0]`), so the bitmap cannot be an addressed
        // RAM: Gowin keeps it in 128 flip-flops plus its 7-to-128 write decode
        // and never reports an SSRAM cell for this module. The previous claim
        // of 128 SSRAM bits was never honoured by the tool.
        Vec::new()
    }

    fn execute_emu(
        _state: &mut Self::EmuState,
        _circuit: &mut CircuitWires,
        _input: &Self::Input,
        _output: &Self::Output,
    ) {
        panic!("data-cache dirty bitmap is Verilog-only")
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
    WriteStream,
    WriteResponse,
    Scan,
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
    // Mirror of the RTL maintenance dirty-line scan: one 16-entry window per
    // cycle, overlapped with the in-flight write-back. `maintenance_dirty`
    // snapshots the dirty bitmap at maintenance start; completed write-backs
    // clear their bit. `scan_index` is the next entry to examine and a found
    // candidate is latched in `found_index` until its write-back starts.
    maintenance_command: Option<crate::MaintenanceCommand>,
    maintenance_dirty: u128,
    scan_active: bool,
    scan_index: u8,
    found_index: Option<u8>,
    wb_index: u8,
    // Mirror of the RTL RAM16 valid-array sweep: reset, a full invalidate, or
    // a memory error clears one set of both ways per cycle; requests are
    // blocked meanwhile, and an invalidate's maintenance_done is delayed
    // until the sweep completes.
    maintenance_invalidate: bool,
    sweep_active: bool,
    sweep_set: u8,
    sweep_finishes_maintenance: bool,
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
            maintenance_command: None,
            maintenance_dirty: 0,
            scan_active: false,
            scan_index: 0,
            found_index: None,
            wb_index: 0,
            maintenance_invalidate: false,
            sweep_active: false,
            sweep_set: 0,
            sweep_finishes_maintenance: false,
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

    /// The lowest dirty entry in the current 16-entry scan window, masked to
    /// entries at or after `scan_index` — mirror of the RTL `scan_masked`
    /// window priority encoder.
    fn scan_window_hit(&self) -> Option<u8> {
        let window = (self.maintenance_dirty >> (self.scan_index & 0x70)) as u16;
        let masked = window & (0xffffu16 << (self.scan_index & 0x0f));
        (masked != 0).then(|| (self.scan_index & 0x70) | masked.trailing_zeros() as u8)
    }

    /// Background scan step during a write-back: latch a found candidate or
    /// advance one 16-entry window.
    fn scan_step(&mut self) {
        if let Some(index) = self.scan_window_hit() {
            self.found_index = Some(index);
            self.scan_active = false;
        } else if self.scan_index >> 4 == 7 {
            self.scan_active = false;
        } else {
            self.scan_index = ((self.scan_index >> 4) + 1) << 4;
        }
    }

    /// Resumes the scan strictly after a consumed candidate.
    fn scan_resume_after(&mut self, index: u8) {
        self.scan_index = index + 1;
        self.scan_active = index != 127;
        self.found_index = None;
    }

    /// Produces the write-back request for a found scan candidate: the first
    /// candidate begins the crate-model maintenance, later ones continue it.
    /// `DataCache::next_maintenance_write` selects lines in the same way-major
    /// order as the RTL window scan, so the request matches the scanned entry.
    fn next_maintenance_request(&mut self) -> crate::MainMemoryRequest {
        if let Some(command) = self.maintenance_command.take() {
            self.cache
                .begin_maintenance(command)
                .expect("idle data cache must accept maintenance")
                .expect("a found scan candidate must have a pending write-back")
        } else {
            self.cache
                .continue_maintenance(crate::MainMemoryResponse::WriteComplete)
                .expect("maintenance completion must match a line write")
                .expect("a found scan candidate must have a pending write-back")
        }
    }

    fn fail_transaction(&mut self) {
        // After a physical-memory error no cache line is allowed to remain
        // architecturally visible: the controller may have accepted an
        // unknown prefix of a burst. The crate model clears instantly; the
        // RTL sweeps its RAM16 valid arrays, which blocks new requests.
        self.cache = crate::DataCache::default();
        self.pending_cpu_request = None;
        self.request = None;
        self.phase = DataMemoryPhase::Idle;
        self.maintenance_command = None;
        self.maintenance_dirty = 0;
        self.scan_active = false;
        self.found_index = None;
        self.sweep_active = true;
        self.sweep_set = 0;
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
            // Mirror of the RTL dirty-write-back edge: the completed line's
            // dirty bit clears before the scan decision.
            self.maintenance_dirty &= !(1u128 << self.wb_index);
            if let Some(index) = self.found_index {
                // The overlapped scan already latched the next dirty line:
                // launch it without a gap.
                let request = self.next_maintenance_request();
                self.wb_index = index;
                self.scan_resume_after(index);
                self.start_request(request);
            } else if self.scan_active {
                self.request = None;
                self.phase = DataMemoryPhase::Scan;
            } else {
                match self
                    .cache
                    .continue_maintenance(crate::MainMemoryResponse::WriteComplete)
                    .expect("maintenance completion must match a line write")
                {
                    Some(_) => unreachable!("scan exhausted but a dirty line remains"),
                    None => {
                        self.request = None;
                        self.phase = DataMemoryPhase::Idle;
                        if self.maintenance_invalidate {
                            self.sweep_active = true;
                            self.sweep_set = 0;
                            self.sweep_finishes_maintenance = true;
                        } else {
                            self.maintenance_active = false;
                            self.maintenance_done = true;
                        }
                    }
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
                    && !input.invalidate_all
                    && !state.sweep_active,
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
                valid_sweep: state.sweep_active,
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
            // The RTL RAM16 valid arrays sweep-clear after reset instead of
            // clearing in one cycle; the system holds the core for the sweep.
            state.sweep_active = true;
            state.sweep_set = 0;
            return;
        }
        state.maintenance_done = false;
        if state.response_valid && input.cpu_response_ready {
            state.response_valid = false;
        }
        // Sweep control: one set of both valid ways cleared per cycle; an
        // invalidate that finished its write-backs reports done when the
        // sweep completes.
        if state.sweep_active {
            if state.sweep_set == 63 {
                state.sweep_active = false;
                state.sweep_set = 0;
                if state.sweep_finishes_maintenance {
                    state.sweep_finishes_maintenance = false;
                    state.maintenance_active = false;
                    state.maintenance_done = true;
                }
            } else {
                state.sweep_set += 1;
            }
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
            state.maintenance_invalidate = input.invalidate_all;
            state.maintenance_command = Some(command);
            state.found_index = None;
            state.maintenance_dirty = state.cache.dirty_bits();
            if state.maintenance_dirty == 0 {
                // No dirty line: a clean completes immediately; an invalidate
                // sweeps the valid arrays and reports done at sweep end.
                match state
                    .cache
                    .begin_maintenance(command)
                    .expect("idle data cache must accept maintenance")
                {
                    Some(_) => unreachable!("empty dirty bitmap produced a write-back"),
                    None => {
                        state.maintenance_command = None;
                        if state.maintenance_invalidate {
                            state.sweep_active = true;
                            state.sweep_set = 0;
                            state.sweep_finishes_maintenance = true;
                        } else {
                            state.maintenance_active = false;
                            state.maintenance_done = true;
                        }
                    }
                }
            } else if state.maintenance_dirty as u16 != 0 {
                // First dirty line sits in window zero: start its write-back
                // immediately and scan on from after it.
                let index = (state.maintenance_dirty as u16).trailing_zeros() as u8;
                let request = state.next_maintenance_request();
                state.wb_index = index;
                state.scan_resume_after(index);
                state.start_request(request);
            } else {
                state.scan_index = 16;
                state.scan_active = true;
                state.phase = DataMemoryPhase::Scan;
            }
            return;
        }

        if state.phase == DataMemoryPhase::Idle
            && !state.response_valid
            && !state.maintenance_active
            && !state.sweep_active
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

        let scan_background = state.scan_active
            && state.found_index.is_none()
            && matches!(
                state.phase,
                DataMemoryPhase::WritebackPrime
                    | DataMemoryPhase::WritebackCapture
                    | DataMemoryPhase::Request
                    | DataMemoryPhase::WriteStream
                    | DataMemoryPhase::WriteResponse
            );
        match state.phase {
            DataMemoryPhase::Idle => {}
            DataMemoryPhase::Lookup => unreachable!(),
            DataMemoryPhase::Scan => {
                if let Some(index) = state.found_index {
                    // Latched by the background scan in the previous cycle.
                    let request = state.next_maintenance_request();
                    state.wb_index = index;
                    state.scan_resume_after(index);
                    state.start_request(request);
                } else if let Some(index) = state.scan_window_hit() {
                    let request = state.next_maintenance_request();
                    state.wb_index = index;
                    state.scan_resume_after(index);
                    state.start_request(request);
                } else if state.scan_index >> 4 == 7 {
                    // The scan found no further dirty line: maintenance ends.
                    state.scan_active = false;
                    if state.maintenance_command.is_some() {
                        unreachable!("nonempty dirty bitmap survived a full scan");
                    }
                    match state
                        .cache
                        .continue_maintenance(crate::MainMemoryResponse::WriteComplete)
                        .expect("maintenance completion must match a line write")
                    {
                        Some(_) => unreachable!("scan exhausted but a dirty line remains"),
                        None => {
                            state.request = None;
                            state.phase = DataMemoryPhase::Idle;
                            if state.maintenance_invalidate {
                                state.sweep_active = true;
                                state.sweep_set = 0;
                                state.sweep_finishes_maintenance = true;
                            } else {
                                state.maintenance_active = false;
                                state.maintenance_done = true;
                            }
                        }
                    }
                } else {
                    state.scan_index = ((state.scan_index >> 4) + 1) << 4;
                }
            }
            DataMemoryPhase::WritebackPrime => {
                state.beat = 0;
                state.phase = DataMemoryPhase::WritebackCapture;
            }
            // The RTL spends a single ST_WB_CAPTURE cycle latching the first
            // writeback beat; the emulator snapshots the whole line in the
            // request, so capture lasts one cycle as well.
            DataMemoryPhase::WritebackCapture => {
                state.phase = DataMemoryPhase::Request;
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
                        // The RTL writes the banks during the receive beats and
                        // responds on the last one; there is no drain phase.
                        state.beat = 0;
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
            DataMemoryPhase::ReadReceive => {}
        }
        // Background scan step, evaluated against pre-edge scan state and
        // applied after the phase logic, exactly like the RTL's separate
        // nonblocking assignment block.
        if scan_background {
            state.scan_step();
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
                )
                .replace(
                    "__CACHE_VALID__",
                    &CpuV3CacheValidRam::verilog_identity().module_name(),
                ),
        )
    }

    fn verilog_dependencies() -> Vec<VerilogDependency> {
        vec![
            VerilogDependency::new::<CpuV3DualPortCacheData<ZeroBsramImage>>("u_data_banks"),
            VerilogDependency::new::<CpuV3CacheTagRam>("u_tags"),
            VerilogDependency::new::<CpuV3DataCacheDirtyRam>("u_dirty"),
            VerilogDependency::new::<CpuV3CacheValidRam>("u_valid"),
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
        let ssram: u64 = resources
            .iter()
            .filter(|resource| resource.kind == ResourceKind::SsramBit)
            .map(|resource| resource.amount)
            .sum();
        assert_eq!(
            ssram,
            (CPU_V3_CACHE_TAG_PHYSICAL_BITS + CPU_V3_CACHE_VALID_PHYSICAL_BITS) as u64
        );
    }

    #[test]
    #[ignore = "explicit external simulation of the CpuV3 two-way cache"]
    fn verify_verilog_with_iverilog() {
        digital_design_hardware::verify_verilog_with_iverilog::<CpuV3TwoWayCache>().unwrap();
    }

    #[test]
    fn data_cache_exports_two_data_bsrams_tag_ssram_and_ff_dirty_bitmap() {
        let project = VerilogProject::generate::<CpuV3DataCache>().unwrap();
        let resources: Vec<_> = project
            .resource_claims
            .iter()
            .flat_map(|claim| claim.resources.iter().copied())
            .collect();
        assert!(resources.contains(&ResourceAmount::new(ResourceKind::Bsram18K, 2)));
        let ssram: u64 = resources
            .iter()
            .filter(|resource| resource.kind == ResourceKind::SsramBit)
            .map(|resource| resource.amount)
            .sum();
        assert_eq!(
            ssram,
            (CPU_V3_CACHE_TAG_PHYSICAL_BITS + CPU_V3_CACHE_VALID_PHYSICAL_BITS) as u64
        );
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
                circuit, input, output, memory, request, false, false, trace,
            );
            if out.cpu_request_ready {
                request = None;
            }
        }
        loop {
            let out = cosim_step(
                circuit, input, output, memory, None, false, false, trace,
            );
            if out.cpu_response_valid {
                let data = out.cpu_read_data;
                cosim_step(
                    circuit, input, output, memory, None, true, false, trace,
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
                circuit, input, output, memory, request, false, false, trace,
            );
            if out.cpu_request_ready {
                request = None;
            }
        }
        loop {
            let out = cosim_step(
                circuit, input, output, memory, None, false, false, trace,
            );
            if out.cpu_response_valid {
                cosim_step(
                    circuit, input, output, memory, None, true, false, trace,
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
                    false,
                    &mut trace,
                );
            }
        }

        // An invalidate racing an outstanding refill may return its old-epoch
        // response, but it must not install that line as a subsequent hit.
        for address in 0x2520..0x2530 {
            memory
                .words
                .insert(address, (0xa000 | (address & 0xff)) as u16);
        }
        let out = cosim_step(
            &mut circuit,
            &input,
            &output,
            &mut memory,
            Some((false, 0x2523, 0)),
            false,
            false,
            &mut trace,
        );
        assert!(out.cpu_request_ready);
        let mut refill_data = None;
        for cycle in 0..40 {
            // Pulse invalidate partway through the refill drain.
            let out = cosim_step(
                &mut circuit,
                &input,
                &output,
                &mut memory,
                None,
                false,
                cycle == 3,
                &mut trace,
            );
            if out.cpu_response_valid {
                refill_data = Some(out.cpu_read_data);
                cosim_step(
                    &mut circuit,
                    &input,
                    &output,
                    &mut memory,
                    None,
                    true,
                    false,
                    &mut trace,
                );
                break;
            }
        }
        assert_eq!(refill_data, Some(0xa023));
        let requests_before = memory.requests;
        assert_eq!(
            cosim_read(
                &mut circuit,
                &input,
                &output,
                &mut memory,
                0x2524,
                &mut trace
            ),
            0xa024
        );
        assert_eq!(
            memory.requests,
            requests_before + 1,
            "invalidate during refill exposed a stale installed line"
        );

        // Invalidate clears the cache through the 64-set valid-array sweep; a
        // second pulse mid-sweep restarts it, and the next read still misses
        // and refetches the line.
        cosim_step(
            &mut circuit,
            &input,
            &output,
            &mut memory,
            None,
            false,
            true,
            &mut trace,
        );
        for _ in 0..3 {
            cosim_step(
                &mut circuit,
                &input,
                &output,
                &mut memory,
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
             reg reset, invalidate_all;\n\
             reg cpu_request_valid, cpu_write, cpu_response_ready;\n\
             reg [31:0] cpu_address;\n\
             reg [15:0] cpu_write_data;\n\
             reg memory_request_ready, memory_response_valid, memory_error;\n\
             reg [63:0] memory_read_data;\n\
             wire cpu_request_ready, cpu_response_valid, cpu_error;\n\
             wire [15:0] cpu_read_data;\n\
             wire memory_request_valid, memory_write, memory_line, memory_response_ready;\n\
             wire [21:0] memory_address;\n\
             wire [63:0] memory_write_data;\n\n\
             {module_name} dut(.*);\n\n\
             always #5 clk = ~clk;\n\n\
             initial begin\n\
                 reset = 1; invalidate_all = 0; cpu_request_valid = 0; cpu_write = 0; cpu_address = 0;\n\
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
                 cpu_response_ready = 1'b{crr}; invalidate_all = 1'b{inv};
\
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
        s.push_str(&CpuV3CacheValidRam::verilog_source().unwrap());
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
