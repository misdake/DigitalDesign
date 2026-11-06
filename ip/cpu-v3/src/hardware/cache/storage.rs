//! Parity-split data BSRAM and synchronous metadata owners.
use super::*;

/// Bank `b` contains words whose word parity is `b`. Way zero occupies the
/// lower 512 addresses and way one occupies the upper 512 addresses.
pub(super) const fn parity_bank_image<I: CpuV3CacheImage, const BANK: bool>() -> [u64; 1024] {
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
pub(super) struct CpuV3DualPortCacheDataInput {
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
pub(super) struct CpuV3DualPortCacheDataOutput {
    bank_0_a_read_data: Wires<16>,
    bank_0_b_read_data: Wires<16>,
    bank_1_a_read_data: Wires<16>,
    bank_1_b_read_data: Wires<16>,
}

pub(super) struct CpuV3DualPortCacheData<I>(PhantomData<I>);

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
localparam [63:0] INITIAL_VALID = 64'h{:016x};
`ifdef __ICARUS__
reg way_0_valid_ram [0:63];
reg way_1_valid_ram [0:63];
reg victim_ram [0:63];
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
`else
// Four explicit 16x1 banks per array make the Gowin RAM16 mapping a hard
// contract.  Inference is otherwise context-sensitive: flattening the data
// cache behind the system arbiter has previously expanded both valid ways
// into 128 FF and roughly 386 LUTs even though this leaf is unchanged.
wire [5:0] valid_write_set = clear_enable ? clear_set : write_set;
wire valid_write_value = clear_enable ? 1'b0 : write_value;
wire way_0_write = clear_enable || (write_enable && !write_way);
wire way_1_write = clear_enable || (write_enable && write_way);
wire [3:0] way_0_bank_data;
wire [3:0] way_1_bank_data;
wire [3:0] victim_bank_data;
genvar bank;
generate
    for (bank = 0; bank < 4; bank = bank + 1) begin : valid_bank
        RAM16SDP1 #(.INIT_0(INITIAL_VALID[bank*16 +: 16])) way_0_ram (
            .DO(way_0_bank_data[bank]), .DI(valid_write_value),
            .WAD(valid_write_set[3:0]), .RAD(read_set[3:0]),
            .WRE(way_0_write && valid_write_set[5:4] == bank), .CLK(clk)
        );
        RAM16SDP1 #(.INIT_0(16'h0000)) way_1_ram (
            .DO(way_1_bank_data[bank]), .DI(valid_write_value),
            .WAD(valid_write_set[3:0]), .RAD(read_set[3:0]),
            .WRE(way_1_write && valid_write_set[5:4] == bank), .CLK(clk)
        );
        RAM16SDP1 #(.INIT_0(16'h0000)) victim_ram (
            .DO(victim_bank_data[bank]), .DI(victim_write_value),
            .WAD(write_set[3:0]), .RAD(read_set[3:0]),
            .WRE(victim_write_enable && write_set[5:4] == bank), .CLK(clk)
        );
    end
endgenerate
assign way_0_valid = way_0_bank_data[read_set[5:4]];
assign way_1_valid = way_1_bank_data[read_set[5:4]];
assign victim = victim_bank_data[read_set[5:4]];
`endif
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
pub(super) struct CpuV3DataCacheMetadataInput {
    read_set: Wires<6>,
    write_0: Wire,
    write_set_0: Wires<6>,
    write_data_0: Wires<16>,
    write_1: Wire,
    write_set_1: Wires<6>,
    write_data_1: Wires<16>,
}

#[derive(Clone, ModuleIo)]
pub(super) struct CpuV3DataCacheMetadataOutput {
    q0: Wires<16>,
    q1: Wires<16>,
}

/// One synchronous record per way/set: tag, valid, dirty and the way-zero victim bit.
pub(super) struct CpuV3CacheMetadataWithImage<I>(PhantomData<I>);
pub(super) type CpuV3DataCacheMetadata = CpuV3CacheMetadataWithImage<ZeroBsramImage>;

impl<I: CpuV3CacheImage> HardwareIdentity for CpuV3CacheMetadataWithImage<I> {
    const TARGET_RESOURCE_LEAF: bool = true;

    fn verilog_identity() -> VerilogIdentity {
        let identity = VerilogIdentity::new("CpuV3DataCacheMetadata").namespace([
            "components",
            "cpu",
            "cpu_v3",
        ]);
        if I::INITIAL_VALID == 0 {
            identity
        } else {
            identity.symbol("IMAGE", format!("v{:016x}", I::INITIAL_VALID))
        }
    }
}

impl<I: CpuV3CacheImage> Module for CpuV3CacheMetadataWithImage<I> {
    type Input = CpuV3DataCacheMetadataInput;
    type Output = CpuV3DataCacheMetadataOutput;
    type EmuState = ();

    const USES_MAIN_CLOCK: bool = true;
    const EMU_AVAILABLE: bool = false;

    fn target_resources() -> Vec<TargetResourceRequest> {
        vec![TargetResourceRequest::new(BsramBlocks::new(1))]
    }

    fn execute_emu(
        _state: &mut Self::EmuState,
        _circuit: &mut CircuitWires,
        _input: &Self::Input,
        _output: &Self::Output,
    ) {
        panic!("metadata BSRAM is verified through the independent cache cycle model")
    }

    fn verilog_source() -> Option<String> {
        // A 16-bit DPB word is addressed by ADA[13:4]; the low four pins
        // select write lanes rather than adding word-address bits. Way zero
        // occupies words 0..63 and holds image validity in bit 12.
        let mut initial_params = String::new();
        for chunk in 0..4 {
            let mut words = [0u16; 16];
            for (lane, word) in words.iter_mut().enumerate() {
                if I::INITIAL_VALID & (1u64 << (chunk * 16 + lane)) != 0 {
                    *word = 0x1000;
                }
            }
            if words.iter().any(|&word| word != 0) {
                let hex: String = words
                    .iter()
                    .rev()
                    .map(|word| format!("{word:04x}"))
                    .collect();
                write!(initial_params, ",.INIT_RAM_{chunk:02X}(256'h{hex})").unwrap();
            }
        }
        Some(
            include_str!("cpu_v3_data_cache_metadata.v")
                .replace(
                    "CpuV3DataCacheMetadata",
                    &Self::verilog_identity().module_name(),
                )
                .replace(
                    "__INITIAL_VALID__",
                    &format!("64'h{:016x}", I::INITIAL_VALID),
                )
                .replace("__INITIAL_PARAMS__", &initial_params),
        )
    }

    fn verilog_testbench() -> Option<String> {
        Some(
            include_str!("cpu_v3_data_cache_metadata_tb.v")
                .replace(
                    "CpuV3DataCacheMetadata",
                    &Self::verilog_identity().module_name(),
                )
                .replace(
                    "__INITIAL_VALID__",
                    &format!("64'h{:016x}", I::INITIAL_VALID),
                ),
        )
    }
}
