use digital_design_circuit::{CircuitWires, Wire, Wires};
use digital_design_hardware::{
    resources::components::BsramBlocks, HardwareIdentity, Module, ModuleIo, TargetResourceRequest,
    VerilogDependency, VerilogIdentity,
};

/// FPU v2 instruction field encodings (fpu-design-v2 sections 6.3, 7 and
/// 10.2): one name per Verilog literal the front-end and the core decode.
/// Testbenches keep their own hand-encoded hex as an independent check.
pub(crate) mod encoding {
    /// Word0 opcode field, bits [15:12].
    pub(crate) const OPCODE_VECTOR: u8 = 0xC;
    pub(crate) const OPCODE_SCALAR: u8 = 0xD;
    pub(crate) const OPCODE_AUX: u8 = 0xE;

    /// Vector word1 subop field, bits [7:3] (section 6.3).
    pub(crate) const VADD: u8 = 0x00;
    pub(crate) const VSUB: u8 = 0x01;
    pub(crate) const VMUL: u8 = 0x02;
    pub(crate) const VMULS: u8 = 0x03;
    pub(crate) const VMIN: u8 = 0x04;
    pub(crate) const VMAX: u8 = 0x05;
    pub(crate) const VABS: u8 = 0x06;
    pub(crate) const VNEG: u8 = 0x07;
    pub(crate) const VMOV: u8 = 0x0C;
    pub(crate) const DOT: u8 = 0x0D;
    pub(crate) const DOTADD: u8 = 0x0E;
    pub(crate) const DOTSTORE: u8 = 0x0F;

    /// Scalar word1 subop field, bits [9:4] (section 7). Only the subops the
    /// scalar, multiply and special paths act on are named here.
    pub(crate) const MUL: u8 = 0x02;
    pub(crate) const CMP: u8 = 0x0B;
    pub(crate) const RCP: u8 = 0x0C;
    pub(crate) const RSQRT: u8 = 0x0D;
    /// SINCOS (Stage 7c): sin -> Fd, cos -> Fd+1. Owned by the special path.
    pub(crate) const SINCOS: u8 = 0x0E;

    /// AUX word1 subop field, bits [9:4], for kind 00 (section 10.2).
    pub(crate) const FLD: u8 = 0x00;
    pub(crate) const FST: u8 = 0x01;

    /// Word0 opcode, bits [15:12].
    pub(crate) fn opcode(word0: u16) -> u8 {
        (word0 >> 12) as u8
    }

    /// Vector/scalar word0 Fa, bits [11:6].
    pub(crate) fn word0_fa(word0: u16) -> u8 {
        ((word0 >> 6) & 0x3F) as u8
    }

    /// Vector/scalar word0 Fb, bits [5:0].
    pub(crate) fn word0_fb(word0: u16) -> u8 {
        (word0 & 0x3F) as u8
    }

    /// AUX word0 Fa, bits [7:2] (bits [1:0] carry the kind).
    pub(crate) fn aux_fa(word0: u16) -> u8 {
        ((word0 >> 2) & 0x3F) as u8
    }

    /// AUX word0 integer register index X, bits [11:8].
    pub(crate) fn aux_x(word0: u16) -> u8 {
        ((word0 >> 8) & 0xF) as u8
    }

    /// AUX word0 kind, bits [1:0].
    pub(crate) fn aux_kind(word0: u16) -> u8 {
        (word0 & 0x3) as u8
    }

    /// Word1 Fd, bits [15:10].
    pub(crate) fn word1_fd(word1: u16) -> u8 {
        ((word1 >> 10) & 0x3F) as u8
    }

    /// Word1 vector length, bits [9:8].
    pub(crate) fn word1_len(word1: u16) -> u8 {
        ((word1 >> 8) & 0x3) as u8
    }

    /// Word1 vector subop, bits [7:3].
    pub(crate) fn vector_subop(word1: u16) -> u8 {
        ((word1 >> 3) & 0x1F) as u8
    }

    /// Word1 scalar or AUX subop, bits [9:4].
    pub(crate) fn scalar_subop(word1: u16) -> u8 {
        ((word1 >> 4) & 0x3F) as u8
    }

    /// Word1 mode field, bits [3:0].
    pub(crate) fn word1_mode(word1: u16) -> u8 {
        (word1 & 0xF) as u8
    }

    /// Vector len -> last-lane index; len 11 is reserved and clamps to vec4.
    pub(crate) fn decoded_last_lane(len_field: u8) -> u8 {
        if len_field == 3 {
            3
        } else {
            len_field + 1
        }
    }
}

pub mod lut;

#[derive(Clone, ModuleIo)]
pub struct CpuV3FpuRegisterRamInput {
    pub write_enable: Wire,
    pub write_address: Wires<9>,
    pub write_data: Wires<32>,
    pub read_a_address: Wires<9>,
    pub read_b_address: Wires<9>,
}

#[derive(Clone, ModuleIo)]
pub struct CpuV3FpuRegisterRamOutput {
    pub read_a_data: Wires<32>,
    pub read_b_data: Wires<32>,
}

/// FPU v2 register file: two mirrored 512x32 BSRAMs (SDPB) giving one
/// broadcast write port plus two independent synchronous read ports.
/// Physical addresses 0..63 alias architectural F0..F63 (the parent forces
/// the top three address bits of architectural accesses to zero); 64..511
/// are the hidden LUT region for RCP/RSQRT/SINCOS. Same-cycle write/read on
/// one address returns the old word on both read ports.
pub struct CpuV3FpuRegisterRam;

impl HardwareIdentity for CpuV3FpuRegisterRam {
    const TARGET_RESOURCE_LEAF: bool = true;

    fn verilog_identity() -> VerilogIdentity {
        VerilogIdentity::new("CpuV3FpuRegisterRam").namespace(["components", "cpu", "cpu_v3"])
    }
}

/// Builds the two register-file BSRAM mirrors from the frozen LUT tables:
/// mirror A carries RCP and SINCOS, mirror B carries the even- and odd-exponent
/// RSQRT tables. The architectural region 0..63 stays zero in both; writes
/// broadcast to both arrays at runtime.
fn rf_mirrors_from_lut() -> (Box<[u32; 512]>, Box<[u32; 512]>) {
    let mut mirror_a = Box::new([0u32; 512]);
    let mut mirror_b = Box::new([0u32; 512]);
    let rcp = lut::packed_rcp_lut();
    let sincos = lut::packed_sincos_lut();
    let rsqrt_even = lut::packed_rsqrt_even_lut();
    let rsqrt_odd = lut::packed_rsqrt_odd_lut();
    mirror_a[lut::MIRROR_A_RCP_BASE..lut::MIRROR_A_RCP_BASE + rcp.len()].copy_from_slice(&rcp);
    mirror_a[lut::MIRROR_A_SINCOS_BASE..lut::MIRROR_A_SINCOS_BASE + sincos.len()]
        .copy_from_slice(&sincos);
    mirror_b[lut::MIRROR_B_RSQRT_EVEN_BASE..lut::MIRROR_B_RSQRT_EVEN_BASE + rsqrt_even.len()]
        .copy_from_slice(&rsqrt_even);
    mirror_b[lut::MIRROR_B_RSQRT_ODD_BASE..lut::MIRROR_B_RSQRT_ODD_BASE + rsqrt_odd.len()]
        .copy_from_slice(&rsqrt_odd);
    (mirror_a, mirror_b)
}

pub struct CpuV3FpuRegisterRamState {
    /// Mirror A (read port A): architectural F0..F63 plus the RCP table at
    /// 128..255 and the SINCOS intervals at 256..511 (design section 9.4).
    mirror_a: Box<[u32; 512]>,
    /// Mirror B (read port B): architectural F0..F63 plus the RSQRT even table
    /// at 128..255 and the RSQRT odd table at 256..383.
    mirror_b: Box<[u32; 512]>,
    read_a_data: u32,
    read_b_data: u32,
}

impl Default for CpuV3FpuRegisterRamState {
    fn default() -> Self {
        let (mirror_a, mirror_b) = rf_mirrors_from_lut();
        CpuV3FpuRegisterRamState {
            mirror_a,
            mirror_b,
            read_a_data: 0,
            read_b_data: 0,
        }
    }
}

impl Module for CpuV3FpuRegisterRam {
    type Input = CpuV3FpuRegisterRamInput;
    type Output = CpuV3FpuRegisterRamOutput;
    type EmuState = CpuV3FpuRegisterRamState;

    const USES_MAIN_CLOCK: bool = true;

    fn target_resources() -> Vec<TargetResourceRequest> {
        vec![TargetResourceRequest::new(BsramBlocks::new(2))]
    }

    fn create_emu(_input: &Self::Input, _output: &Self::Output) -> Self::EmuState {
        Self::EmuState::default()
    }

    fn execute_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        _input: &Self::Input,
        output: &Self::Output,
    ) {
        output.drive(
            circuit,
            &CpuV3FpuRegisterRamOutputValue {
                read_a_data: u64::from(state.read_a_data),
                read_b_data: u64::from(state.read_b_data),
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
        // Both reads observe the pre-write contents (read-first semantics); the
        // write broadcasts the same word into both mirrors.
        state.read_a_data = state.mirror_a[input.read_a_address as usize];
        state.read_b_data = state.mirror_b[input.read_b_address as usize];
        if input.write_enable {
            state.mirror_a[input.write_address as usize] = input.write_data as u32;
            state.mirror_b[input.write_address as usize] = input.write_data as u32;
        }
    }

    fn verilog_source() -> Option<String> {
        Some(include_str!("cpu_v3_fpu_register_ram.v").to_string())
    }

    fn verilog_testbench() -> Option<String> {
        Some(include_str!("cpu_v3_fpu_register_ram_tb.v").to_string())
    }
}

#[derive(Clone, ModuleIo)]
pub struct CpuV3FpuFrontendInput {
    pub word_valid: Wire,
    pub word: Wires<16>,
    pub abort: Wire,
}

#[derive(Clone, ModuleIo)]
pub struct CpuV3FpuFrontendOutput {
    pub read_valid: Wire,
    pub rf_read_a_address: Wires<9>,
    pub rf_read_b_address: Wires<9>,
    pub word0_raw: Wires<16>,
    pub word1_raw: Wires<16>,
    pub instr_opcode: Wires<4>,
    pub instr_complete: Wire,
}

/// FPU v2 instruction front-end: assembles the 32-bit instruction from two
/// 16-bit words and forms the register-file read addresses combinationally
/// in the word0 cycle so the synchronous RF presents operands one cycle
/// later, before word1 is decoded. Word1 field splitting (len/subop/mode)
/// is left to the downstream controller; this leaf only latches raw halves.
#[derive(Default)]
pub struct CpuV3FpuFrontend;

impl HardwareIdentity for CpuV3FpuFrontend {
    const TARGET_RESOURCE_LEAF: bool = true;

    fn verilog_identity() -> VerilogIdentity {
        VerilogIdentity::new("CpuV3FpuFrontend").namespace(["components", "cpu", "cpu_v3"])
    }
}

#[derive(Default)]
pub struct CpuV3FpuFrontendState {
    waiting_word1: bool,
    word0_raw: u16,
    word1_raw: u16,
    instr_opcode: u8,
    instr_complete: bool,
}

impl Module for CpuV3FpuFrontend {
    type Input = CpuV3FpuFrontendInput;
    type Output = CpuV3FpuFrontendOutput;
    type EmuState = CpuV3FpuFrontendState;

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
        let input = input.sample(circuit);
        let word = input.word as u16;
        let read_valid = input.word_valid && !state.waiting_word1;
        // AUX word0 carries Fa at bits [7:2]; the other opcodes at [11:6].
        let read_a = if encoding::opcode(word) == encoding::OPCODE_AUX {
            encoding::aux_fa(word)
        } else {
            encoding::word0_fa(word)
        };
        output.drive(
            circuit,
            &CpuV3FpuFrontendOutputValue {
                read_valid,
                rf_read_a_address: u64::from(read_a),
                rf_read_b_address: u64::from(encoding::word0_fb(word)),
                word0_raw: u64::from(state.word0_raw),
                word1_raw: u64::from(state.word1_raw),
                instr_opcode: u64::from(state.instr_opcode),
                instr_complete: state.instr_complete,
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
        let word = input.word as u16;
        let accept_word0 = input.word_valid && !state.waiting_word1;
        let accept_word1 = input.word_valid && state.waiting_word1 && !input.abort;
        let discard_word0 = input.abort && state.waiting_word1;
        state.instr_complete = accept_word1;
        if accept_word0 {
            state.word0_raw = word;
            state.instr_opcode = encoding::opcode(word);
            state.waiting_word1 = true;
        } else if accept_word1 || discard_word0 {
            state.waiting_word1 = false;
        }
        if accept_word1 {
            state.word1_raw = word;
        }
    }

    fn verilog_source() -> Option<String> {
        Some(include_str!("cpu_v3_fpu_frontend.v").to_string())
    }

    fn verilog_testbench() -> Option<String> {
        Some(include_str!("cpu_v3_fpu_frontend_tb.v").to_string())
    }
}

#[derive(Clone, ModuleIo)]
pub struct CpuV3FpuScalarAluInput {
    pub a: Wires<32>,
    pub b: Wires<32>,
    pub op: Wires<4>,
}

#[derive(Clone, ModuleIo)]
pub struct CpuV3FpuScalarAluOutput {
    pub result: Wires<32>,
    pub flag_lt: Wire,
    pub flag_eq: Wire,
    pub flag_gt: Wire,
}

/// FPU v2 scalar Q16.16 ALU: purely combinational, all arithmetic in fabric
/// LUTs (syn_dspstyle="logic"; DSP absorption of wide adds is a measured
/// Gowin pitfall). Wrap overflow policy, round-half-up ROUND; flags are only
/// specified for op == CMP.
pub struct CpuV3FpuScalarAlu;

impl HardwareIdentity for CpuV3FpuScalarAlu {
    const TARGET_RESOURCE_LEAF: bool = true;

    fn verilog_identity() -> VerilogIdentity {
        VerilogIdentity::new("CpuV3FpuScalarAlu").namespace(["components", "cpu", "cpu_v3"])
    }
}

impl Module for CpuV3FpuScalarAlu {
    type Input = CpuV3FpuScalarAluInput;
    type Output = CpuV3FpuScalarAluOutput;
    type EmuState = ();

    fn create_emu(_input: &Self::Input, _output: &Self::Output) -> Self::EmuState {}

    fn execute_emu(
        _state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        input: &Self::Input,
        output: &Self::Output,
    ) {
        let input = input.sample(circuit);
        let a = input.a as u32;
        let b = input.b as u32;
        let sa = a as i32;
        let sb = b as i32;
        let floor_a = a & 0xFFFF_0000;
        let frac_nonzero = a & 0xFFFF != 0;
        let ceil_a = floor_a.wrapping_add(if frac_nonzero { 0x1_0000 } else { 0 });
        let result = match input.op & 0xF {
            0x0 => a.wrapping_add(b),
            0x1 => a.wrapping_sub(b),
            0x3 => {
                if sa < sb {
                    a
                } else {
                    b
                }
            }
            0x4 => {
                if sa > sb {
                    a
                } else {
                    b
                }
            }
            0x5 => {
                if sa < 0 {
                    a.wrapping_neg()
                } else {
                    a
                }
            }
            0x6 => a.wrapping_neg(),
            0x7 => floor_a,
            0x8 => ceil_a,
            0x9 => a.wrapping_add(0x8000) & 0xFFFF_0000,
            0xA => {
                if sa < 0 {
                    ceil_a
                } else {
                    floor_a
                }
            }
            0xB => 0,
            0xF => a,
            _ => 0,
        };
        output.drive(
            circuit,
            &CpuV3FpuScalarAluOutputValue {
                result: u64::from(result),
                flag_lt: sa < sb,
                flag_eq: sa == sb,
                flag_gt: sa > sb,
            },
        );
    }

    fn verilog_source() -> Option<String> {
        Some(include_str!("cpu_v3_fpu_scalar_alu.v").to_string())
    }

    fn verilog_testbench() -> Option<String> {
        Some(include_str!("cpu_v3_fpu_scalar_alu_tb.v").to_string())
    }
}

#[derive(Clone, ModuleIo)]
pub struct CpuV3FpuScalarPathInput {
    pub abort: Wire,
    pub instr_complete: Wire,
    pub instr_opcode: Wires<4>,
    pub word1_raw: Wires<16>,
    pub rf_read_a_data: Wires<32>,
    pub rf_read_b_data: Wires<32>,
}

#[derive(Clone, ModuleIo)]
pub struct CpuV3FpuScalarPathOutput {
    pub rf_write_enable: Wire,
    pub rf_write_address: Wires<9>,
    pub rf_write_data: Wires<32>,
    pub flag_lt: Wire,
    pub flag_eq: Wire,
    pub flag_gt: Wire,
    pub busy: Wire,
    pub r_wait: Wires<4>,
    pub w_wait: Wires<4>,
    pub x_wait: Wires<4>,
}

/// FPU v2 scalar execution-path controller: on instr_complete (opcode 0xD)
/// it evaluates the combinational scalar ALU on the already-valid RF read
/// data (T0), captures the result, writes back on T1, and drives the
/// section-16 R/W/X countdown skeleton. CMP only updates the flag registers;
/// abort cancels any in-flight write.
pub struct CpuV3FpuScalarPath;

impl HardwareIdentity for CpuV3FpuScalarPath {
    const TARGET_RESOURCE_LEAF: bool = false;

    fn verilog_identity() -> VerilogIdentity {
        VerilogIdentity::new("CpuV3FpuScalarPath").namespace(["components", "cpu", "cpu_v3"])
    }
}

#[derive(Default)]
pub struct CpuV3FpuScalarPathState {
    write_enable: bool,
    write_address: u16,
    write_data: u32,
    flag_lt: bool,
    flag_eq: bool,
    flag_gt: bool,
    w_count: u8,
    x_count: u8,
}

impl CpuV3FpuScalarPathState {
    /// Mirrors CpuV3FpuScalarAlu: wrap-only Q16.16 ops.
    fn alu(a: u32, b: u32, op: u8) -> (u32, bool, bool, bool) {
        let sa = a as i32;
        let sb = b as i32;
        let floor_a = a & 0xFFFF_0000;
        let frac = a & 0xFFFF != 0;
        let ceil_a = floor_a.wrapping_add(if frac { 0x1_0000 } else { 0 });
        let result = match op & 0xF {
            0x0 => a.wrapping_add(b),
            0x1 => a.wrapping_sub(b),
            0x3 => {
                if sa < sb {
                    a
                } else {
                    b
                }
            }
            0x4 => {
                if sa > sb {
                    a
                } else {
                    b
                }
            }
            0x5 => {
                if sa < 0 {
                    a.wrapping_neg()
                } else {
                    a
                }
            }
            0x6 => a.wrapping_neg(),
            0x7 => floor_a,
            0x8 => ceil_a,
            0x9 => a.wrapping_add(0x8000) & 0xFFFF_0000,
            0xA => {
                if sa < 0 {
                    ceil_a
                } else {
                    floor_a
                }
            }
            0xF => a,
            _ => 0,
        };
        (result, sa < sb, sa == sb, sa > sb)
    }
}

impl Module for CpuV3FpuScalarPath {
    type Input = CpuV3FpuScalarPathInput;
    type Output = CpuV3FpuScalarPathOutput;
    type EmuState = CpuV3FpuScalarPathState;

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
        let input = input.sample(circuit);
        let load_now = input.instr_complete
            && input.instr_opcode == u64::from(encoding::OPCODE_SCALAR)
            && !input.abort;
        let w_wait = if load_now { 2 } else { state.w_count };
        let x_wait = if load_now { 2 } else { state.x_count };
        output.drive(
            circuit,
            &CpuV3FpuScalarPathOutputValue {
                rf_write_enable: state.write_enable && !input.abort,
                rf_write_address: u64::from(state.write_address),
                rf_write_data: u64::from(state.write_data),
                flag_lt: state.flag_lt,
                flag_eq: state.flag_eq,
                flag_gt: state.flag_gt,
                busy: w_wait != 0,
                r_wait: 0,
                w_wait: u64::from(w_wait),
                x_wait: u64::from(x_wait),
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
        if input.abort {
            state.write_enable = false;
            state.w_count = 0;
            state.x_count = 0;
            return;
        }
        let word1 = input.word1_raw as u16;
        let subop = encoding::scalar_subop(word1);
        let fd = encoding::word1_fd(word1);
        let is_cmp = subop == encoding::CMP;
        // MUL is owned by the multiply path; never fire here.
        let load_now = input.instr_complete
            && input.instr_opcode == u64::from(encoding::OPCODE_SCALAR)
            && subop != encoding::MUL;
        state.write_enable = load_now && !is_cmp;
        if load_now {
            let (result, lt, eq, gt) = Self::EmuState::alu(
                input.rf_read_a_data as u32,
                input.rf_read_b_data as u32,
                subop,
            );
            state.write_address = u16::from(fd);
            state.write_data = result;
            if is_cmp {
                state.flag_lt = lt;
                state.flag_eq = eq;
                state.flag_gt = gt;
            }
            state.w_count = 1;
            state.x_count = 1;
        } else {
            state.w_count = state.w_count.saturating_sub(1);
            state.x_count = state.x_count.saturating_sub(1);
        }
    }

    fn verilog_source() -> Option<String> {
        Some(include_str!("cpu_v3_fpu_scalar_path.v").to_string())
    }

    fn verilog_dependencies() -> Vec<VerilogDependency> {
        vec![VerilogDependency::new::<CpuV3FpuScalarAlu>("scalar_alu")]
    }

    fn verilog_testbench() -> Option<String> {
        Some(include_str!("cpu_v3_fpu_scalar_path_tb.v").to_string())
    }
}

#[derive(Clone, ModuleIo)]
pub struct CpuV3FpuSpecialPathInput {
    pub abort: Wire,
    pub instr_complete: Wire,
    pub instr_opcode: Wires<4>,
    pub word1_raw: Wires<16>,
    pub rf_read_a_data: Wires<32>,
    pub rf_read_b_data: Wires<32>,
    pub mul_out_valid: Wire,
    pub mul_out_product: Wires<64>,
    pub mul_out_tag: Wires<9>,
}

#[derive(Clone, ModuleIo)]
pub struct CpuV3FpuSpecialPathOutput {
    pub rf_read_a_address: Wires<9>,
    pub rf_read_b_address: Wires<9>,
    pub rf_write_enable: Wire,
    pub rf_write_address: Wires<9>,
    pub rf_write_data: Wires<32>,
    pub busy: Wire,
    pub r_wait: Wires<4>,
    pub w_wait: Wires<4>,
    pub x_wait: Wires<4>,
    pub mul_in_valid: Wire,
    pub mul_in_a: Wires<36>,
    pub mul_in_b: Wires<36>,
    pub mul_in_tag: Wires<9>,
}

/// FPU v2 special-function execution-path controller (opcode 0xD subops 0x0C
/// RCP, 0x0D RSQRT and 0x0E SINCOS). Blocking: while active it owns both
/// register-file read ports. Emu lives in the unit top; the leaf is verified
/// through its Verilog testbench and the system co-simulation.
pub struct CpuV3FpuSpecialPath;

impl HardwareIdentity for CpuV3FpuSpecialPath {
    const TARGET_RESOURCE_LEAF: bool = true;

    fn verilog_identity() -> VerilogIdentity {
        VerilogIdentity::new("CpuV3FpuSpecialPath").namespace(["components", "cpu", "cpu_v3"])
    }
}

/// Pipeline registers of the blocking special path. Instruction context is
/// written once and remains stable while only the valid token/data advance.
#[derive(Default)]
pub struct CpuV3FpuSpecialPathState {
    p0_valid: bool,
    p0_rcp: bool,
    p0_negative: bool,
    p0_magnitude: u32,
    p0_fd: u8,
    s1_valid: bool,
    s1_residue: u16,
    s1_shift: u8,
    s1_left: bool,
    s1_zero: bool,
    s2_valid: bool,
    s2_interpolated: u32,
    // SINCOS context (Stage 7c): the range product comes from the shared
    // 36x36 pipe; this context only registers the returned quadrant/fraction,
    // the mode controls and the interpolation result.
    sc_valid: bool,
    sc_stage: u8,
    sc_fd: u8,
    sc_mode: u8,
    sc_q: u8,
    sc_f: u16,
    sc_result: u32,
}

/// The positive SINCOS range-reduction constant `round((2/pi) * 2^32)`.
/// `C0*2^16 + C1 == K`, so `(a * K) >>> 32` is the same integer as the frozen
/// two-term reducer `h0 + ((((p0 + h1) << 16) + p1) >> 32)` for every i32 `a`.
const SINCOS_K: i64 = 2_734_261_102;

/// All combinational results of one cycle, sampled from the pre-edge state.
/// `comb` turns the output subset into wires; `tick` uses the capture subset to
/// update the pipeline registers. Both must see the same pre-edge snapshot.
struct CpuV3FpuSpecialPathComputed {
    load_now: bool,
    is_rcp: bool,
    is_sincos: bool,
    p0_negative: bool,
    p0_magnitude: u32,
    read_a_address: u16,
    read_b_address: u16,
    write_enable: bool,
    write_address: u16,
    write_data: u32,
    busy: bool,
    r_wait: u8,
    w_wait: u8,
    x_wait: u8,
    mul_in_valid: bool,
    mul_in_a: i64,
    mul_in_b: i64,
    mul_in_tag: u16,
    s1_residue: u16,
    s1_shift: u8,
    s1_left: bool,
    s1_zero: bool,
    capture_interpolated: u32,
    fd: u8,
    sc_mode: u8,
    sc_q_next: u8,
    sc_f_next: u16,
    sc_result_next: u32,
}

impl CpuV3FpuSpecialPathState {
    /// Mirrors every combinational assignment of CpuV3FpuSpecialPath from the
    /// pre-edge state and the live inputs.
    fn compute(&self, input: &CpuV3FpuSpecialPathInputValue) -> CpuV3FpuSpecialPathComputed {
        let instr_opcode = input.instr_opcode as u8;
        let word1 = input.word1_raw as u16;
        let subop = encoding::scalar_subop(word1);
        let fd = encoding::word1_fd(word1);
        let mode = encoding::word1_mode(word1) & 0x3;
        let is_scalar = instr_opcode == encoding::OPCODE_SCALAR;
        let is_rcp = is_scalar && subop == encoding::RCP;
        let is_rsqrt = is_scalar && subop == encoding::RSQRT;
        let is_sincos = is_scalar && subop == encoding::SINCOS;
        let load_now = input.instr_complete && (is_rcp || is_rsqrt || is_sincos) && !input.abort;

        // T0 operand capture; normalization runs from the registered operand.
        let x0 = input.rf_read_a_data as u32;
        let x0_negative = x0 & 0x8000_0000 != 0;
        let x0_magnitude = if x0_negative { x0.wrapping_neg() } else { x0 };
        let p0_zero = self.p0_magnitude == 0;
        let clz = self.p0_magnitude.leading_zeros();

        // Shared normalized index/residue for both tables.
        let normalized = self.p0_magnitude.wrapping_shl(clz);
        let norm_index = ((normalized >> 24) & 0x7F) as u8;
        let norm_residue = ((normalized >> 15) & 0x1FF) as u16;

        // RCP scale: 2^(clz-15).
        let rcp_shift = clz.abs_diff(15) as u8;
        let rcp_left = clz >= 15;

        // RSQRT parity is parity(31-clz). Its scale is floor((15-clz)/2).
        let rsqrt_odd = clz & 1 == 0;
        let rsqrt_left = clz > 15;
        let rsqrt_distance = clz.abs_diff(15);
        let rsqrt_shift = if rsqrt_left {
            rsqrt_distance.div_ceil(2)
        } else {
            rsqrt_distance / 2
        };

        // Aligned bases make the address a concatenation, not an addition.
        let table_address = if self.p0_rcp || !rsqrt_odd {
            128u16 | u16::from(norm_index)
        } else {
            256u16 | u16::from(norm_index)
        };

        // SINCOS reduced-argument reflection uses the registered quadrant and
        // fraction the shared multiply pipe returned.
        let sc_u_sin = if self.sc_q & 1 == 1 {
            0x1_0000 - i64::from(self.sc_f)
        } else {
            i64::from(self.sc_f)
        };
        let sc_u_cos = if self.sc_q & 1 == 1 {
            i64::from(self.sc_f)
        } else {
            0x1_0000 - i64::from(self.sc_f)
        };
        let sc_single = matches!(self.sc_mode, 1 | 2);
        let sc_first_is_cos = self.sc_mode == 2;
        let sc_interp_is_cos = sc_first_is_cos || (!sc_single && self.sc_stage == 6);
        let sc_interp_u = if sc_interp_is_cos { sc_u_cos } else { sc_u_sin };
        let sc_residue = sc_interp_u & 0xFF;

        // One packed read supplies both the 17-bit current and signed 10-bit
        // delta. The product fits signed 19 bits, but i32 keeps the emu clear.
        let packed_interval = if self.p0_rcp {
            input.rf_read_a_data as u32
        } else {
            input.rf_read_b_data as u32
        };
        let interval_current = packed_interval & 0x1_FFFF;
        let delta_bits = ((packed_interval >> 17) & 0x03FF) as i32;
        let interval_delta = (delta_bits << 22) >> 22;

        // SINCOS packed interval (always mirror A). The local 18x18 multiplier
        // only interpolates now; the range product comes from the shared pipe.
        let sc_delta_bits = ((input.rf_read_a_data as u32 >> 17) & 0x03FF) as i32;
        let sc_delta = ((sc_delta_bits << 22) >> 22) as i64;
        let mul_x = if self.sc_valid {
            sc_delta
        } else {
            i64::from(interval_delta)
        };
        let mul_y = if self.sc_valid {
            sc_residue
        } else {
            i64::from(self.s1_residue)
        };
        let mul_product = mul_x.wrapping_mul(mul_y);
        let interpolated = (interval_current as i32 + (mul_product >> 9) as i32) as u32;

        // SINCOS range product: issued at T0, returned by the shared 36x36
        // pipe at T3. phase = (signed(Fa) * K) >>> 32; only phase[17:0] is
        // consumed. The shift is its own binding, never inside a ternary.
        let sc_phase_shifted = (input.mul_out_product as i64) >> 32;
        let sc_phase18 = (sc_phase_shifted as u64) & 0x3_FFFF;
        let sc_q_comb = ((sc_phase18 >> 16) & 0x3) as u8;
        let sc_f_comb = (sc_phase18 & 0xFFFF) as u16;

        // Shared-pipe drive: one signed-Fa-by-K product at T0. The signed-32
        // operand sign-extends to the pipe's 36-bit bus; K is the positive
        // constant (the pipe's i64 model is exact).
        let mul_in_valid = load_now && is_sincos;
        let mul_in_a = i64::from(x0 as i32);
        let mul_in_b = SINCOS_K;
        let mul_in_tag = 0u16;

        // T4 drives the selected first address; dual-output mode drives the
        // cosine address at T5 while interpolating sine.
        let sc_addr_u = if self.sc_stage == 5 || sc_first_is_cos {
            sc_u_cos
        } else {
            sc_u_sin
        };
        let sc_lut_address = 256u16 + ((sc_addr_u as u32 & 0xFFFF) >> 8) as u16;

        let read_a_address = if self.p0_valid && self.p0_rcp {
            table_address
        } else if self.sc_valid && (self.sc_stage == 4 || (!sc_single && self.sc_stage == 5)) {
            sc_lut_address
        } else {
            0
        };
        let read_b_address = if self.p0_valid && !self.p0_rcp {
            table_address
        } else {
            0
        };

        // SINCOS interpolation (T5 first result, T6 second) and result sign.
        let sc_current = (input.rf_read_a_data as u32) & 0x1_FFFF;
        let sc_interp_sum = i64::from(sc_current) + (mul_product >> 8);
        let sc_interp_mag = if sc_interp_u & 0x1_0000 != 0 {
            0x1_0000
        } else {
            (sc_interp_sum & 0x1_FFFF) as u32
        };
        let sc_result_sign = if sc_interp_is_cos {
            ((self.sc_q >> 1) & 1) ^ (self.sc_q & 1)
        } else {
            (self.sc_q >> 1) & 1
        };
        let sc_result: u32 = if sc_result_sign == 1 {
            (-(sc_interp_mag as i32)) as u32
        } else {
            sc_interp_mag
        };

        // T3 scaling and sign. Only RCP's left path can overflow signed 32;
        // RSQRT's maximum left shift is eight.
        let scaled = if self.s1_left {
            let wide = u64::from(self.s2_interpolated) << self.s1_shift;
            if self.p0_rcp && wide > 0x7FFF_FFFF {
                0x7FFF_FFFF
            } else {
                wide as u32
            }
        } else {
            self.s2_interpolated >> self.s1_shift
        };
        let rcp_value = if self.p0_negative {
            scaled.wrapping_neg()
        } else {
            scaled
        };
        let out_rcp = if self.s1_zero {
            if self.p0_negative {
                0x8000_0001
            } else {
                0x7FFF_FFFF
            }
        } else {
            rcp_value
        };
        let out_rsqrt = if self.s1_zero { 0 } else { scaled };
        let rcp_result = if self.p0_rcp { out_rcp } else { out_rsqrt };

        // The interpolation is registered before the RF port: T6 writes the
        // first result and T7 writes the second. The 6-bit index wraps like the
        // RTL concatenation.
        let sc_write = self.sc_valid
            && (self.sc_stage == 6 || (!sc_single && self.sc_stage == 7))
            && !input.abort;
        let write_enable = (self.s2_valid || sc_write) && !input.abort;
        let write_address = if sc_write {
            u16::from(self.sc_fd.wrapping_add(u8::from(self.sc_stage == 7)) & 0x3F)
        } else {
            u16::from(self.p0_fd)
        };
        let write_data = if sc_write { self.sc_result } else { rcp_result };

        // SINCOS capture values. q/f come from the pipe product returned at
        // T3; the interpolation result registers at T5/T6.
        let sc_q_next = if self.sc_stage == 3 {
            sc_q_comb
        } else {
            self.sc_q
        };
        let sc_f_next = if self.sc_stage == 3 {
            sc_f_comb
        } else {
            self.sc_f
        };

        CpuV3FpuSpecialPathComputed {
            load_now,
            is_rcp,
            is_sincos,
            p0_negative: x0_negative,
            p0_magnitude: x0_magnitude,
            read_a_address,
            read_b_address,
            write_enable,
            write_address,
            write_data,
            busy: (load_now || self.p0_valid || self.s1_valid || self.s2_valid || self.sc_valid)
                && !input.abort,
            r_wait: 0,
            w_wait: if load_now {
                if is_sincos {
                    if matches!(mode, 1 | 2) {
                        7
                    } else {
                        8
                    }
                } else {
                    4
                }
            } else {
                0
            },
            x_wait: if load_now {
                if is_sincos {
                    if matches!(mode, 1 | 2) {
                        7
                    } else {
                        8
                    }
                } else {
                    4
                }
            } else {
                0
            },
            mul_in_valid,
            mul_in_a,
            mul_in_b,
            mul_in_tag,
            s1_residue: norm_residue,
            s1_shift: if self.p0_rcp {
                rcp_shift
            } else {
                rsqrt_shift as u8
            },
            s1_left: if self.p0_rcp { rcp_left } else { rsqrt_left },
            s1_zero: if self.p0_rcp {
                p0_zero
            } else {
                self.p0_negative || p0_zero
            },
            capture_interpolated: interpolated,
            fd,
            sc_mode: mode,
            sc_q_next,
            sc_f_next,
            sc_result_next: sc_result,
        }
    }

    fn comb(&self, input: &CpuV3FpuSpecialPathInputValue) -> CpuV3FpuSpecialPathOutputValue {
        let c = self.compute(input);
        CpuV3FpuSpecialPathOutputValue {
            rf_read_a_address: u64::from(c.read_a_address),
            rf_read_b_address: u64::from(c.read_b_address),
            rf_write_enable: c.write_enable,
            rf_write_address: u64::from(c.write_address),
            rf_write_data: u64::from(c.write_data),
            busy: c.busy,
            r_wait: u64::from(c.r_wait),
            w_wait: u64::from(c.w_wait),
            x_wait: u64::from(c.x_wait),
            mul_in_valid: c.mul_in_valid,
            mul_in_a: c.mul_in_a as u64,
            mul_in_b: c.mul_in_b as u64,
            mul_in_tag: u64::from(c.mul_in_tag),
        }
    }

    /// Mirrors the single always block of CpuV3FpuSpecialPath. Reads only the
    /// pre-edge snapshot in `c` plus the pre-edge register fields it mutates.
    fn tick(&mut self, input: &CpuV3FpuSpecialPathInputValue) {
        let c = self.compute(input);
        if input.abort {
            self.p0_valid = false;
            self.s1_valid = false;
            self.s2_valid = false;
            self.sc_valid = false;
            self.sc_stage = 0;
        } else if c.load_now {
            if c.is_sincos {
                self.sc_valid = true;
                self.sc_stage = 1;
                self.sc_fd = c.fd;
                self.sc_mode = c.sc_mode;
                self.p0_valid = false;
                self.s1_valid = false;
                self.s2_valid = false;
            } else {
                self.sc_valid = false;
                self.p0_valid = true;
                self.p0_rcp = c.is_rcp;
                self.p0_negative = c.p0_negative;
                self.p0_magnitude = c.p0_magnitude;
                self.p0_fd = c.fd;
                self.s1_valid = false;
                self.s2_valid = false;
            }
        } else if self.sc_valid {
            // Keep the pre-edge stage explicit: the RTL nonblocking assignments
            // below this case all test the old value, even when the case clears
            // sc_stage on the same edge.
            let sc_stage = self.sc_stage;
            let sc_single = matches!(self.sc_mode, 1 | 2);
            match sc_stage {
                3 => {
                    self.sc_q = c.sc_q_next;
                    self.sc_f = c.sc_f_next;
                }
                5 => self.sc_result = c.sc_result_next,
                6 => {
                    if sc_single {
                        self.sc_valid = false;
                        self.sc_stage = 0;
                    } else {
                        self.sc_result = c.sc_result_next;
                    }
                }
                7 => {
                    self.sc_valid = false;
                    self.sc_stage = 0;
                }
                _ => {}
            }
            if sc_stage != 7 && !(sc_stage == 6 && sc_single) {
                self.sc_stage = sc_stage + 1;
            }
        } else if self.p0_valid {
            self.p0_valid = false;
            self.s1_valid = true;
            self.s1_residue = c.s1_residue;
            self.s1_shift = c.s1_shift;
            self.s1_left = c.s1_left;
            self.s1_zero = c.s1_zero;
            self.s2_valid = false;
        } else if self.s1_valid {
            self.s2_valid = true;
            self.s2_interpolated = c.capture_interpolated;
            self.s1_valid = false;
        } else {
            self.s2_valid = false;
        }
    }
}

impl Module for CpuV3FpuSpecialPath {
    type Input = CpuV3FpuSpecialPathInput;
    type Output = CpuV3FpuSpecialPathOutput;
    type EmuState = CpuV3FpuSpecialPathState;

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
        let input = input.sample(circuit);
        output.drive(circuit, &state.comb(&input));
    }

    fn clock_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        input: &Self::Input,
        _output: &Self::Output,
    ) {
        let input = input.sample(circuit);
        state.tick(&input);
    }

    fn verilog_source() -> Option<String> {
        Some(include_str!("cpu_v3_fpu_special_path.v").to_string())
    }

    fn target_resources() -> Vec<TargetResourceRequest> {
        // The one inferred 18x18 signed multiplier is time-shared by the
        // RCP/RSQRT/SINCOS interpolation products. SINCOS range reduction now
        // reuses the shared 36x36 pipe, so no further lane is claimed here.
        vec![TargetResourceRequest::new(
            digital_design_hardware::resources::components::DspMultipliers::new(1),
        )]
    }

    fn verilog_testbench() -> Option<String> {
        Some(include_str!("cpu_v3_fpu_special_path_tb.v").to_string())
    }
}

#[derive(Clone, ModuleIo)]
pub struct CpuV3FpuVectorPathInput {
    pub abort: Wire,
    pub instr_complete: Wire,
    pub instr_opcode: Wires<4>,
    pub word1_raw: Wires<16>,
    pub base_a: Wires<6>,
    pub base_b: Wires<6>,
    pub rf_read_a_data: Wires<32>,
    pub rf_read_b_data: Wires<32>,
}

#[derive(Clone, ModuleIo)]
pub struct CpuV3FpuVectorPathOutput {
    pub rf_read_a_address: Wires<9>,
    pub rf_read_b_address: Wires<9>,
    pub rf_write_enable: Wire,
    pub rf_write_address: Wires<9>,
    pub rf_write_data: Wires<32>,
    pub busy: Wire,
}

/// FPU v2 vector execution path (opcode 0xC): one lane per cycle through
/// the shared combinational scalar ALU. Emu lives in the unit top
/// (CpuV3FpuState); this leaf is verified through its Verilog testbench.
pub struct CpuV3FpuVectorPath;

impl HardwareIdentity for CpuV3FpuVectorPath {
    const TARGET_RESOURCE_LEAF: bool = false;

    fn verilog_identity() -> VerilogIdentity {
        VerilogIdentity::new("CpuV3FpuVectorPath").namespace(["components", "cpu", "cpu_v3"])
    }
}

impl Module for CpuV3FpuVectorPath {
    type Input = CpuV3FpuVectorPathInput;
    type Output = CpuV3FpuVectorPathOutput;
    type EmuState = ();

    const USES_MAIN_CLOCK: bool = true;
    const EMU_AVAILABLE: bool = false;

    fn verilog_source() -> Option<String> {
        Some(include_str!("cpu_v3_fpu_vector_path.v").to_string())
    }

    fn verilog_dependencies() -> Vec<VerilogDependency> {
        vec![VerilogDependency::new::<CpuV3FpuScalarAlu>("vector_alu")]
    }

    fn verilog_testbench() -> Option<String> {
        Some(include_str!("cpu_v3_fpu_vector_path_tb.v").to_string())
    }
}

#[derive(Clone, ModuleIo)]
pub struct CpuV3FpuMultiplyPathInput {
    pub abort: Wire,
    pub instr_complete: Wire,
    pub instr_opcode: Wires<4>,
    pub word1_raw: Wires<16>,
    pub base_a: Wires<6>,
    pub base_b: Wires<6>,
    pub rf_read_a_data: Wires<32>,
    pub rf_read_b_data: Wires<32>,
    pub mul_out_valid: Wire,
    pub mul_out_product: Wires<64>,
    pub mul_out_tag: Wires<9>,
}

#[derive(Clone, ModuleIo)]
pub struct CpuV3FpuMultiplyPathOutput {
    pub mul_in_valid: Wire,
    pub mul_in_a: Wires<36>,
    pub mul_in_b: Wires<36>,
    pub mul_in_tag: Wires<9>,
    pub rf_read_a_address: Wires<9>,
    pub rf_read_b_address: Wires<9>,
    pub rf_write_enable: Wire,
    pub rf_write_address: Wires<9>,
    pub rf_write_data: Wires<32>,
    pub busy: Wire,
}

/// FPU v2 multiply execution path (VMUL/VMULS/scalar MUL): one lane per
/// cycle through an inferred 36x36 signed multiplier, three register stages,
/// narrowing to Q16.16 at the write port. Emu lives in the unit top.
pub struct CpuV3FpuMultiplyPath;

impl HardwareIdentity for CpuV3FpuMultiplyPath {
    const TARGET_RESOURCE_LEAF: bool = true;

    fn verilog_identity() -> VerilogIdentity {
        VerilogIdentity::new("CpuV3FpuMultiplyPath").namespace(["components", "cpu", "cpu_v3"])
    }
}

impl Module for CpuV3FpuMultiplyPath {
    type Input = CpuV3FpuMultiplyPathInput;
    type Output = CpuV3FpuMultiplyPathOutput;
    type EmuState = ();

    const USES_MAIN_CLOCK: bool = true;
    const EMU_AVAILABLE: bool = false;

    // The multiplier lives in the shared CpuV3FpuMulPipe leaf; this
    // controller claims nothing itself.
    fn verilog_source() -> Option<String> {
        Some(include_str!("cpu_v3_fpu_multiply_path.v").to_string())
    }

    fn verilog_testbench() -> Option<String> {
        Some(include_str!("cpu_v3_fpu_multiply_path_tb.v").to_string())
    }
}

#[derive(Clone, ModuleIo)]
pub struct CpuV3FpuMulPipeInput {
    pub abort: Wire,
    pub in_valid: Wire,
    pub in_a: Wires<36>,
    pub in_b: Wires<36>,
    pub in_tag: Wires<9>,
}

#[derive(Clone, ModuleIo)]
pub struct CpuV3FpuMulPipeOutput {
    pub out_valid: Wire,
    pub out_product: Wires<64>,
    pub out_tag: Wires<9>,
}

/// Shared FPU v2 36x36 multiply pipeline (one MULT36X36 = four 18x18 DSP
/// lanes): three register stages, tag-carrying FIFO. The multiply and dot
/// paths share it because the core serializes instructions. Emu lives in the
/// unit top; this leaf is verified through its Verilog testbench.
pub struct CpuV3FpuMulPipe;

impl HardwareIdentity for CpuV3FpuMulPipe {
    const TARGET_RESOURCE_LEAF: bool = true;

    fn verilog_identity() -> VerilogIdentity {
        VerilogIdentity::new("CpuV3FpuMulPipe").namespace(["components", "cpu", "cpu_v3"])
    }
}

impl Module for CpuV3FpuMulPipe {
    type Input = CpuV3FpuMulPipeInput;
    type Output = CpuV3FpuMulPipeOutput;
    type EmuState = ();

    const USES_MAIN_CLOCK: bool = true;
    const EMU_AVAILABLE: bool = false;

    fn target_resources() -> Vec<TargetResourceRequest> {
        vec![TargetResourceRequest::new(
            digital_design_hardware::resources::components::DspMultipliers::new(4),
        )]
    }

    fn verilog_source() -> Option<String> {
        Some(include_str!("cpu_v3_fpu_mul_pipe.v").to_string())
    }

    fn verilog_testbench() -> Option<String> {
        Some(include_str!("cpu_v3_fpu_mul_pipe_tb.v").to_string())
    }
}

#[derive(Clone, ModuleIo)]
pub struct CpuV3FpuDotPathInput {
    pub abort: Wire,
    pub instr_complete: Wire,
    pub instr_opcode: Wires<4>,
    pub word1_raw: Wires<16>,
    pub base_a: Wires<6>,
    pub base_b: Wires<6>,
    pub rf_read_a_data: Wires<32>,
    pub rf_read_b_data: Wires<32>,
    pub mul_out_valid: Wire,
    pub mul_out_product: Wires<64>,
    pub mul_out_tag: Wires<9>,
}

#[derive(Clone, ModuleIo)]
pub struct CpuV3FpuDotPathOutput {
    pub mul_in_valid: Wire,
    pub mul_in_a: Wires<36>,
    pub mul_in_b: Wires<36>,
    pub mul_in_tag: Wires<9>,
    pub rf_read_a_address: Wires<9>,
    pub rf_read_b_address: Wires<9>,
    pub rf_write_enable: Wire,
    pub rf_write_address: Wires<9>,
    pub rf_write_data: Wires<32>,
    pub busy: Wire,
    pub acc_out: Wires<64>,
}

/// FPU v2 dot-product path (DOT/DOTADD/DOTSTORE, opcode 0xC): per-lane full
/// 72-bit products accumulate into a 64-bit Q32.32 fabric ACC (LUT adder,
/// syn_dspstyle="logic"), narrowing only at DOTSTORE. Emu lives in the unit
/// top; this leaf is verified through its Verilog testbench.
pub struct CpuV3FpuDotPath;

impl HardwareIdentity for CpuV3FpuDotPath {
    const TARGET_RESOURCE_LEAF: bool = true;

    fn verilog_identity() -> VerilogIdentity {
        VerilogIdentity::new("CpuV3FpuDotPath").namespace(["components", "cpu", "cpu_v3"])
    }
}

impl Module for CpuV3FpuDotPath {
    type Input = CpuV3FpuDotPathInput;
    type Output = CpuV3FpuDotPathOutput;
    type EmuState = ();

    const USES_MAIN_CLOCK: bool = true;
    const EMU_AVAILABLE: bool = false;

    fn target_resources() -> Vec<TargetResourceRequest> {
        // The 64-bit accumulate maps to a MULTADDALU18X18 macro (a full
        // macro, two lanes) even though the multiplier moved into the shared
        // pipe leaf; the fusion is deliberate, see the leaf's header comment.
        vec![TargetResourceRequest::new(
            digital_design_hardware::resources::components::DspMultipliers::new(2),
        )]
    }

    fn verilog_source() -> Option<String> {
        Some(include_str!("cpu_v3_fpu_dot_path.v").to_string())
    }

    fn verilog_testbench() -> Option<String> {
        Some(include_str!("cpu_v3_fpu_dot_path_tb.v").to_string())
    }
}

#[derive(Clone, ModuleIo)]
pub struct CpuV3FpuInput {
    pub abort: Wire,
    pub word_valid: Wire,
    pub word: Wires<16>,
    pub ext_access: Wire,
    pub ext_write_enable: Wire,
    pub ext_write_address: Wires<9>,
    pub ext_write_data: Wires<32>,
    pub ext_read_address: Wires<9>,
}

#[derive(Clone, ModuleIo)]
pub struct CpuV3FpuOutput {
    pub busy: Wire,
    pub flag_lt: Wire,
    pub flag_eq: Wire,
    pub flag_gt: Wire,
    pub instr_complete: Wire,
    pub ext_read_data: Wires<32>,
}

/// FPU v2 unit top level: two-word front-end + mirrored-BSRAM register
/// file + scalar execution path, plus the external memory channel the core
/// uses for FLD/FST until the internal store buffer arrives (Stage 6).
/// Integration contract: fpu-design-v2 section 26.
pub struct CpuV3Fpu;

impl HardwareIdentity for CpuV3Fpu {
    const TARGET_RESOURCE_LEAF: bool = false;

    fn verilog_identity() -> VerilogIdentity {
        VerilogIdentity::new("CpuV3Fpu").namespace(["components", "cpu", "cpu_v3"])
    }
}

/// Compositional emu state: the three leaf states plus the one-beat
/// operand-address hold register.
pub struct CpuV3FpuState {
    frontend: CpuV3FpuFrontendState,
    rf: CpuV3FpuRegisterRamState,
    scalar_path: CpuV3FpuScalarPathState,
    special_path: CpuV3FpuSpecialPathState,
    held_read_a_address: u16,
    held_read_b_address: u16,
    vp_run: bool,
    vp_lane: u8,
    vp_last_lane: u8,
    vp_base_a: u8,
    vp_base_b: u8,
    vp_fd: u8,
    vp_wr_enable: bool,
    vp_wr_address: u16,
    vp_wr_data: u32,
    mp_run: bool,
    mp_lane: u8,
    mp_last_lane: u8,
    mp_base_a: u8,
    mp_base_b: u8,
    mp_is_vmuls: bool,
    mp_waddr: u16,
    mp_outstanding: u8,
    dp_run: bool,
    dp_lane: u8,
    dp_last_lane: u8,
    dp_base_a: u8,
    dp_base_b: u8,
    dp_stride: u8,
    dp_store_mode: bool,
    dp_acc_lane: u8,
    dp_store: bool,
    dp_store_addr: u16,
    dp_store_data: u32,
    dp_outstanding: u8,
    dp_acc: i64,
    // Shared multiply pipe (CpuV3FpuMulPipe): the only multiplier in the
    // unit. The tag carries the multiply path's write address; the dot path
    // tags lanes for observability.
    pipe_s1_valid: bool,
    pipe_s1_a: i64,
    pipe_s1_b: i64,
    pipe_s1_tag: u16,
    pipe_s2_valid: bool,
    pipe_s2_prod: i64,
    pipe_s2_tag: u16,
    pipe_s3_valid: bool,
    pipe_s3_prod: i64,
    pipe_s3_tag: u16,
}

impl Default for CpuV3FpuState {
    fn default() -> Self {
        CpuV3FpuState {
            frontend: CpuV3FpuFrontendState::default(),
            rf: CpuV3FpuRegisterRamState::default(),
            scalar_path: CpuV3FpuScalarPathState::default(),
            special_path: CpuV3FpuSpecialPathState::default(),
            held_read_a_address: 0,
            held_read_b_address: 0,
            vp_run: false,
            vp_lane: 0,
            vp_last_lane: 0,
            vp_base_a: 0,
            vp_base_b: 0,
            vp_fd: 0,
            vp_wr_enable: false,
            vp_wr_address: 0,
            vp_wr_data: 0,
            mp_run: false,
            mp_lane: 0,
            mp_last_lane: 0,
            mp_base_a: 0,
            mp_base_b: 0,
            mp_is_vmuls: false,
            mp_waddr: 0,
            mp_outstanding: 0,
            dp_run: false,
            dp_lane: 0,
            dp_last_lane: 0,
            dp_base_a: 0,
            dp_base_b: 0,
            dp_stride: 1,
            dp_store_mode: false,
            dp_acc_lane: 0,
            dp_store: false,
            dp_store_addr: 0,
            dp_store_data: 0,
            dp_outstanding: 0,
            dp_acc: 0,
            pipe_s1_valid: false,
            pipe_s1_a: 0,
            pipe_s1_b: 0,
            pipe_s1_tag: 0,
            pipe_s2_valid: false,
            pipe_s2_prod: 0,
            pipe_s2_tag: 0,
            pipe_s3_valid: false,
            pipe_s3_prod: 0,
            pipe_s3_tag: 0,
        }
    }
}

impl CpuV3FpuState {
    /// Combinational outputs of the unit top (registered leaf outputs and the
    /// busy/complete status); register updates live in `tick`.
    pub(crate) fn comb(&self, input: &CpuV3FpuInputValue) -> CpuV3FpuOutputValue {
        let sp = &self.scalar_path;
        let sp_load_now = self.frontend.instr_complete
            && self.frontend.instr_opcode == encoding::OPCODE_SCALAR
            && !matches!(
                encoding::scalar_subop(self.frontend.word1_raw),
                encoding::MUL | encoding::RCP | encoding::RSQRT | encoding::SINCOS
            )
            && !input.abort;
        let sp_w_wait = if sp_load_now { 2 } else { sp.w_count };
        let vp_load_now = self.frontend.instr_complete
            && self.frontend.instr_opcode == encoding::OPCODE_VECTOR
            && !input.abort
            && matches!(
                encoding::vector_subop(self.frontend.word1_raw),
                encoding::VADD
                    | encoding::VSUB
                    | encoding::VMIN
                    | encoding::VMAX
                    | encoding::VABS
                    | encoding::VNEG
                    | encoding::VMOV
            );
        let vp_busy = (vp_load_now || self.vp_run || self.vp_wr_enable) && !input.abort;
        let mp_load_now = self.frontend.instr_complete
            && !input.abort
            && ((self.frontend.instr_opcode == encoding::OPCODE_VECTOR
                && matches!(
                    encoding::vector_subop(self.frontend.word1_raw),
                    encoding::VMUL | encoding::VMULS
                ))
                || (self.frontend.instr_opcode == encoding::OPCODE_SCALAR
                    && encoding::scalar_subop(self.frontend.word1_raw) == encoding::MUL));
        let mp_busy =
            (mp_load_now || self.mp_run || self.mp_outstanding != 0 || self.pipe_s3_valid)
                && !input.abort;
        let dp_load_now = self.frontend.instr_complete
            && !input.abort
            && self.frontend.instr_opcode == encoding::OPCODE_VECTOR
            && matches!(
                encoding::vector_subop(self.frontend.word1_raw),
                encoding::DOT | encoding::DOTADD | encoding::DOTSTORE
            );
        let dp_busy = (dp_load_now
            || self.dp_run
            || self.dp_outstanding != 0
            || self.pipe_s3_valid
            || self.dp_store)
            && !input.abort;
        // Special path (RCP/RSQRT/SINCOS): blocking, so its busy window is
        // exactly the T0..T3 or T0..T7 pipeline its own state tracks.
        let sf_load_now = self.frontend.instr_complete
            && self.frontend.instr_opcode == encoding::OPCODE_SCALAR
            && matches!(
                encoding::scalar_subop(self.frontend.word1_raw),
                encoding::RCP | encoding::RSQRT | encoding::SINCOS
            )
            && !input.abort;
        let sf_busy = (sf_load_now
            || self.special_path.p0_valid
            || self.special_path.s1_valid
            || self.special_path.s2_valid
            || self.special_path.sc_valid)
            && !input.abort;
        CpuV3FpuOutputValue {
            busy: sp_w_wait != 0 || vp_busy || mp_busy || dp_busy || sf_busy,
            flag_lt: self.scalar_path.flag_lt,
            flag_eq: self.scalar_path.flag_eq,
            flag_gt: self.scalar_path.flag_gt,
            instr_complete: self.frontend.instr_complete,
            ext_read_data: u64::from(self.rf.read_b_data),
        }
    }
}

impl Module for CpuV3Fpu {
    type Input = CpuV3FpuInput;
    type Output = CpuV3FpuOutput;
    type EmuState = CpuV3FpuState;

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
        let input = input.sample(circuit);
        output.drive(circuit, &state.comb(&input));
    }

    fn clock_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        input: &Self::Input,
        _output: &Self::Output,
    ) {
        let input = input.sample(circuit);
        state.tick(&input);
    }

    fn verilog_source() -> Option<String> {
        Some(include_str!("cpu_v3_fpu.v").to_string())
    }

    fn verilog_dependencies() -> Vec<VerilogDependency> {
        vec![
            VerilogDependency::new::<CpuV3FpuFrontend>("frontend"),
            VerilogDependency::new::<CpuV3FpuRegisterRam>("rf"),
            VerilogDependency::new::<CpuV3FpuScalarPath>("scalar_path"),
            VerilogDependency::new::<CpuV3FpuSpecialPath>("special_path"),
            VerilogDependency::new::<CpuV3FpuVectorPath>("vector_path"),
            VerilogDependency::new::<CpuV3FpuMultiplyPath>("multiply_path"),
            VerilogDependency::new::<CpuV3FpuDotPath>("dot_path"),
            VerilogDependency::new::<CpuV3FpuMulPipe>("mul_pipe"),
        ]
    }

    fn verilog_testbench() -> Option<String> {
        Some(include_str!("cpu_v3_fpu_tb.v").to_string())
    }
}

impl CpuV3FpuState {
    /// Register updates for one cycle. The core emu also drives the unit
    /// through `comb`/`tick` directly (value-level, no wires).
    pub(crate) fn tick(&mut self, input: &CpuV3FpuInputValue) {
        let state = self;
        let word = input.word as u16;
        // Snapshot the pre-edge instr_complete: the scalar path below must
        // observe the value the RTL presents during this cycle, not the value
        // the front-end register update is about to write (nonblocking
        // semantics; the leaf-level Verilog testbenches cannot catch this).
        let instr_complete_prev = state.frontend.instr_complete;

        // Pre-edge special-path snapshot. The leaf's combinational logic and
        // its always block both read these values, exactly as the RTL reads its
        // inputs and register outputs during one cycle. `sp_comb` gives the
        // RF port muxes the same addresses and write the RTL presents.
        let sp_input = CpuV3FpuSpecialPathInputValue {
            abort: input.abort,
            instr_complete: instr_complete_prev,
            instr_opcode: u64::from(state.frontend.instr_opcode),
            word1_raw: u64::from(state.frontend.word1_raw),
            rf_read_a_data: u64::from(state.rf.read_a_data),
            rf_read_b_data: u64::from(state.rf.read_b_data),
            // The shared pipe's stage-3 output is the special path's range
            // product at its T3; abort voids it combinationally.
            mul_out_valid: state.pipe_s3_valid && !input.abort,
            mul_out_product: state.pipe_s3_prod as u64,
            mul_out_tag: u64::from(state.pipe_s3_tag),
        };
        let sp_comb = state.special_path.comb(&sp_input);

        // Front-end register updates (identical to the leaf clock_emu).
        let accept_word0 = input.word_valid && !state.frontend.waiting_word1;
        let accept_word1 = input.word_valid && state.frontend.waiting_word1 && !input.abort;
        let discard_word0 = input.abort && state.frontend.waiting_word1;
        state.frontend.instr_complete = accept_word1;
        if accept_word0 {
            state.frontend.word0_raw = word;
            state.frontend.instr_opcode = encoding::opcode(word);
            state.frontend.waiting_word1 = true;
            // The wrapper captures the read addresses on the word0 beat; AUX
            // word0 carries Fa at bits [7:2], the other opcodes at [11:6].
            state.held_read_a_address =
                u16::from(if encoding::opcode(word) == encoding::OPCODE_AUX {
                    encoding::aux_fa(word)
                } else {
                    encoding::word0_fa(word)
                });
            state.held_read_b_address = u16::from(encoding::word0_fb(word));
        } else if accept_word1 || discard_word0 {
            state.frontend.waiting_word1 = false;
        }
        if accept_word1 {
            state.frontend.word1_raw = word;
        }

        // Scalar path register updates (identical to the leaf clock_emu).
        let sp = &mut state.scalar_path;
        if input.abort {
            sp.write_enable = false;
            sp.w_count = 0;
            sp.x_count = 0;
        } else {
            let word1 = state.frontend.word1_raw;
            let subop = encoding::scalar_subop(word1);
            let fd = encoding::word1_fd(word1);
            let is_cmp = subop == encoding::CMP;
            // MUL is owned by the multiply path; RCP/RSQRT/SINCOS by the
            // special path.
            let load_now = instr_complete_prev
                && state.frontend.instr_opcode == encoding::OPCODE_SCALAR
                && !matches!(
                    subop,
                    encoding::MUL | encoding::RCP | encoding::RSQRT | encoding::SINCOS
                );
            // read-first: the RF read registers still hold T0 operands here.
            sp.write_enable = load_now && !is_cmp;
            if load_now {
                let (result, lt, eq, gt) =
                    CpuV3FpuScalarPathState::alu(state.rf.read_a_data, state.rf.read_b_data, subop);
                sp.write_address = u16::from(fd);
                sp.write_data = result;
                if is_cmp {
                    sp.flag_lt = lt;
                    sp.flag_eq = eq;
                    sp.flag_gt = gt;
                }
                sp.w_count = 1;
                sp.x_count = 1;
            } else {
                sp.w_count = sp.w_count.saturating_sub(1);
                sp.x_count = sp.x_count.saturating_sub(1);
            }
        }

        // Read addresses for this cycle's RF read: computed from pre-edge
        // vector state (RTL nonblocking semantics), so they are resolved
        // before the vector register updates below.
        let word1_fields = state.frontend.word1_raw;
        let vp_load_now = instr_complete_prev
            && state.frontend.instr_opcode == encoding::OPCODE_VECTOR
            && matches!(
                encoding::vector_subop(word1_fields),
                encoding::VADD
                    | encoding::VSUB
                    | encoding::VMIN
                    | encoding::VMAX
                    | encoding::VABS
                    | encoding::VNEG
                    | encoding::VMOV
            );
        let dp_load_now = instr_complete_prev
            && state.frontend.instr_opcode == encoding::OPCODE_VECTOR
            && matches!(
                encoding::vector_subop(word1_fields),
                encoding::DOT | encoding::DOTADD | encoding::DOTSTORE
            );
        let dp_active = dp_load_now || state.dp_run;
        let mp_load_now = instr_complete_prev
            && ((state.frontend.instr_opcode == encoding::OPCODE_VECTOR
                && matches!(
                    encoding::vector_subop(word1_fields),
                    encoding::VMUL | encoding::VMULS
                ))
                || (state.frontend.instr_opcode == encoding::OPCODE_SCALAR
                    && encoding::scalar_subop(word1_fields) == encoding::MUL));
        let mp_active = mp_load_now || state.mp_run;
        let vp_active = vp_load_now || state.vp_run;
        let read_a_address = if sp_comb.busy {
            sp_comb.rf_read_a_address as usize
        } else if dp_load_now {
            usize::from(encoding::word0_fa(state.frontend.word0_raw))
        } else if dp_active && state.dp_lane <= state.dp_last_lane {
            usize::from(state.dp_base_a) + usize::from(state.dp_lane)
        } else if dp_active {
            0
        } else if mp_load_now {
            usize::from(encoding::word0_fa(state.frontend.word0_raw))
        } else if mp_active && state.mp_lane <= state.mp_last_lane {
            usize::from(state.mp_base_a) + usize::from(state.mp_lane)
        } else if mp_active {
            0
        } else if vp_load_now {
            usize::from(encoding::word0_fa(state.frontend.word0_raw))
        } else if vp_active && state.vp_lane <= state.vp_last_lane {
            usize::from(state.vp_base_a) + usize::from(state.vp_lane)
        } else if vp_active {
            0
        } else {
            state.held_read_a_address as usize
        };
        let read_b_address = if sp_comb.busy {
            sp_comb.rf_read_b_address as usize
        } else if dp_load_now {
            usize::from(encoding::word0_fb(state.frontend.word0_raw))
        } else if dp_active && state.dp_lane <= state.dp_last_lane {
            usize::from(state.dp_base_b) + usize::from(state.dp_lane) * usize::from(state.dp_stride)
        } else if dp_active {
            0
        } else if mp_load_now {
            usize::from(encoding::word0_fb(state.frontend.word0_raw))
        } else if mp_active && state.mp_lane <= state.mp_last_lane {
            usize::from(state.mp_base_b)
                + if state.mp_is_vmuls {
                    0
                } else {
                    usize::from(state.mp_lane)
                }
        } else if mp_active {
            0
        } else if vp_load_now {
            usize::from(encoding::word0_fb(state.frontend.word0_raw))
        } else if vp_active && state.vp_lane <= state.vp_last_lane {
            usize::from(state.vp_base_b) + usize::from(state.vp_lane)
        } else if vp_active {
            0
        } else if input.ext_access {
            input.ext_read_address as usize
        } else {
            state.held_read_b_address as usize
        };

        // Shared pre-edge snapshots for the four per-path tick helpers. Every
        // value is sampled before any path register update, mirroring the
        // RTL's nonblocking assignment semantics; the helpers must not
        // re-sample state they mutate.
        let ctx = TickContext {
            abort: input.abort,
            instr_opcode: state.frontend.instr_opcode,
            word0_raw: state.frontend.word0_raw,
            word1_fields,
            len_field: encoding::word1_len(word1_fields),
            vector_subop: encoding::vector_subop(word1_fields),
            vp_load_now,
            mp_load_now,
            dp_load_now,
            mp_data_valid: state.mp_run
                && state.mp_lane >= 1
                && (state.mp_lane - 1) <= state.mp_last_lane,
            dp_data_valid: state.dp_run
                && state.dp_lane >= 1
                && (state.dp_lane - 1) <= state.dp_last_lane,
            pipe_out_valid: state.pipe_s3_valid && !input.abort,
            pipe_out_product: state.pipe_s3_prod,
            pipe_out_tag: state.pipe_s3_tag,
            // Pre-edge counts: the RTL guards the accumulate and the RF write
            // with the count from before this edge's return is removed, so a
            // returning final entry still counts as owned by the path.
            mp_outstanding_prev: state.mp_outstanding,
            dp_outstanding_prev: state.dp_outstanding,
            // Pre-edge lane bookkeeping for the pipe tags: the RTL presents
            // waddr_r / lane_r combinationally and only increments them at the
            // edge, so the tag must be sampled before the updates below.
            mp_waddr_prev: state.mp_waddr,
            dp_lane_prev: state.dp_lane,
            dp_store_prev: state.dp_store,
            rf_read_a: state.rf.read_a_data,
            rf_read_b: state.rf.read_b_data,
            // Special-path shared-pipe drive (SINCOS range product at its T0).
            sf_mul_in_valid: sp_comb.mul_in_valid,
            sf_mul_in_a: sp_comb.mul_in_a as i64,
            sf_mul_in_b: sp_comb.mul_in_b as i64,
            sf_mul_in_tag: sp_comb.mul_in_tag as u16,
        };

        // Vector path register updates (mirrors CpuV3FpuVectorPath).
        state.tick_vector_path(&ctx);

        // Multiply path register updates (mirrors CpuV3FpuMultiplyPath):
        // the lane sequencer hands operand pairs to the shared pipe with the
        // destination tag; the pipe returns them three cycles later.
        state.tick_multiply_path(&ctx);

        // Dot path register updates (mirrors CpuV3FpuDotPath). ACC
        // accumulates the pipe product returning this cycle (pre-edge stage
        // three); the DOTSTORE final-lane capture and the writeback clear
        // read the pre-edge store flag.
        state.tick_dot_path(&ctx);

        // Shared multiply pipe transfer (mirrors CpuV3FpuMulPipe): reverse
        // order so every stage reads its predecessor's pre-edge value.
        state.tick_mul_pipe(&ctx);

        // Special-function path register updates (mirrors CpuV3FpuSpecialPath):
        // the pre-edge snapshot was taken at the top of this function.
        state.tick_special_path(&sp_input);

        // Register-file updates last: reads sample the pre-write contents.
        let rf = &mut state.rf;
        rf.read_a_data = rf.mirror_a[read_a_address];
        rf.read_b_data = rf.mirror_b[read_b_address];
        let sp = &state.scalar_path;
        let (we, wa, wd) = if input.ext_access {
            (
                input.ext_write_enable,
                input.ext_write_address as usize,
                input.ext_write_data as u32,
            )
        } else if state.vp_wr_enable && !input.abort {
            (true, state.vp_wr_address as usize, state.vp_wr_data)
        } else if ctx.pipe_out_valid && ctx.mp_outstanding_prev != 0 && !input.abort {
            (
                true,
                ctx.pipe_out_tag as usize,
                ((ctx.pipe_out_product >> 16) & 0xFFFF_FFFF) as u32,
            )
        } else if ctx.dp_store_prev && !input.abort {
            (true, state.dp_store_addr as usize, state.dp_store_data)
        } else if sp_comb.rf_write_enable {
            (
                true,
                sp_comb.rf_write_address as usize,
                sp_comb.rf_write_data as u32,
            )
        } else {
            (
                sp.write_enable && !input.abort,
                sp.write_address as usize,
                sp.write_data,
            )
        };
        if we {
            rf.mirror_a[wa] = wd;
            rf.mirror_b[wa] = wd;
        }
    }
}

/// Pre-edge snapshots shared by the four per-path tick helpers. `tick` samples
/// every value before the first path update, mirroring the RTL's nonblocking
/// assignment semantics; each helper then owns exactly one execution path's
/// register block and must not re-sample state it mutates.
struct TickContext {
    abort: bool,
    instr_opcode: u8,
    word0_raw: u16,
    word1_fields: u16,
    len_field: u8,
    vector_subop: u8,
    vp_load_now: bool,
    mp_load_now: bool,
    dp_load_now: bool,
    mp_data_valid: bool,
    dp_data_valid: bool,
    pipe_out_valid: bool,
    pipe_out_product: i64,
    pipe_out_tag: u16,
    mp_outstanding_prev: u8,
    dp_outstanding_prev: u8,
    mp_waddr_prev: u16,
    dp_lane_prev: u8,
    dp_store_prev: bool,
    rf_read_a: u32,
    rf_read_b: u32,
    // Special-path shared-pipe drive (SINCOS range product): sf wins the input
    // multiplexer because the core never overlaps the owners.
    sf_mul_in_valid: bool,
    sf_mul_in_a: i64,
    sf_mul_in_b: i64,
    sf_mul_in_tag: u16,
}

impl CpuV3FpuState {
    /// Vector-path register updates: one RTL always block of
    /// CpuV3FpuVectorPath. Reads only the pre-edge operands in `ctx`.
    fn tick_vector_path(&mut self, ctx: &TickContext) {
        let last_lane = encoding::decoded_last_lane(ctx.len_field);
        // subop -> scalar ALU op; the seven supported subops select the exact
        // leaf entries, unlisted subops select the leaf's defined zero.
        let alu_op = match ctx.vector_subop {
            encoding::VADD => 0x0,
            encoding::VSUB => 0x1,
            encoding::VMIN => 0x3,
            encoding::VMAX => 0x4,
            encoding::VABS => 0x5,
            encoding::VNEG => 0x6,
            encoding::VMOV => 0xF,
            _ => 0x2,
        };
        if ctx.abort {
            self.vp_run = false;
            self.vp_wr_enable = false;
        } else if ctx.vp_load_now {
            self.vp_run = true;
            self.vp_lane = 1;
            self.vp_last_lane = last_lane;
            self.vp_base_a = encoding::word0_fa(ctx.word0_raw);
            self.vp_base_b = encoding::word0_fb(ctx.word0_raw);
            self.vp_fd = encoding::word1_fd(ctx.word1_fields);
            self.vp_wr_enable = false;
        } else if self.vp_run {
            let data_valid = self.vp_lane >= 1 && (self.vp_lane - 1) <= self.vp_last_lane;
            self.vp_wr_enable = data_valid;
            if data_valid {
                let data_lane = self.vp_lane - 1;
                let (result, _, _, _) =
                    CpuV3FpuScalarPathState::alu(ctx.rf_read_a, ctx.rf_read_b, alu_op);
                self.vp_wr_address = self.vp_fd as u16 + u16::from(data_lane);
                self.vp_wr_data = result;
            }
            if self.vp_lane > self.vp_last_lane + 1 {
                self.vp_run = false;
            } else {
                self.vp_lane += 1;
            }
        } else {
            self.vp_wr_enable = false;
        }
    }

    /// Multiply-path register updates: one RTL always block of
    /// CpuV3FpuMultiplyPath. The lane sequencer hands operand pairs to the
    /// shared pipe with the destination tag; the pipe returns them three
    /// cycles later. Reads only the pre-edge operands in `ctx`.
    fn tick_multiply_path(&mut self, ctx: &TickContext) {
        if ctx.abort {
            self.mp_run = false;
            self.mp_outstanding = 0;
        } else if ctx.mp_load_now {
            self.mp_run = true;
            self.mp_lane = 1;
            self.mp_last_lane = if ctx.instr_opcode == encoding::OPCODE_SCALAR {
                0
            } else {
                encoding::decoded_last_lane(ctx.len_field)
            };
            self.mp_base_a = encoding::word0_fa(ctx.word0_raw);
            self.mp_base_b = encoding::word0_fb(ctx.word0_raw);
            self.mp_is_vmuls =
                ctx.instr_opcode == encoding::OPCODE_VECTOR && ctx.vector_subop == encoding::VMULS;
            self.mp_waddr = u16::from(encoding::word1_fd(ctx.word1_fields));
            self.mp_outstanding = 0;
        } else {
            if self.mp_run {
                if self.mp_lane > self.mp_last_lane + 1 {
                    self.mp_run = false;
                } else {
                    self.mp_lane += 1;
                }
                self.mp_waddr = self.mp_waddr.wrapping_add(1);
            }
            // Only this path's own returns decrement (the pipe is shared).
            self.mp_outstanding = self
                .mp_outstanding
                .wrapping_add(u8::from(ctx.mp_data_valid))
                .wrapping_sub(u8::from(ctx.pipe_out_valid && self.mp_outstanding != 0));
        }
    }

    /// Dot-path register updates: one RTL always block of CpuV3FpuDotPath.
    /// ACC accumulates the pipe product returning this cycle (pre-edge stage
    /// three); the DOTSTORE final-lane capture and the writeback clear read the
    /// pre-edge store flag. Reads only the pre-edge operands in `ctx`.
    fn tick_dot_path(&mut self, ctx: &TickContext) {
        if ctx.abort {
            self.dp_run = false;
            self.dp_outstanding = 0;
            self.dp_store_mode = false;
            self.dp_store = false;
        } else if ctx.dp_load_now {
            self.dp_run = true;
            self.dp_lane = 1;
            self.dp_last_lane = encoding::decoded_last_lane(ctx.len_field);
            self.dp_base_a = encoding::word0_fa(ctx.word0_raw);
            self.dp_base_b = encoding::word0_fb(ctx.word0_raw);
            self.dp_stride = match encoding::word1_mode(ctx.word1_fields) & 0x3 {
                1 => 3,
                2 => 4,
                _ => 1,
            };
            self.dp_store_mode = ctx.vector_subop == encoding::DOTSTORE;
            self.dp_acc_lane = 0;
            self.dp_store = false;
            self.dp_store_addr = u16::from(encoding::word1_fd(ctx.word1_fields));
            self.dp_outstanding = 0;
            // DOT and DOTSTORE start a fresh sum; DOTADD continues the ACC.
            if matches!(ctx.vector_subop, encoding::DOT | encoding::DOTSTORE) {
                self.dp_acc = 0;
            }
        } else {
            if self.dp_run {
                if self.dp_lane > self.dp_last_lane + 1 {
                    self.dp_run = false;
                } else {
                    self.dp_lane += 1;
                }
            }
            self.dp_outstanding = self
                .dp_outstanding
                .wrapping_add(u8::from(ctx.dp_data_valid))
                .wrapping_sub(u8::from(ctx.pipe_out_valid && self.dp_outstanding != 0));
            // Accumulate the returning product. Only entries this instruction
            // issued count (the pipe is shared with the multiply path).
            if ctx.pipe_out_valid && ctx.dp_outstanding_prev != 0 {
                let sum = self.dp_acc.wrapping_add(ctx.pipe_out_product);
                self.dp_acc = sum;
                if self.dp_store_mode && self.dp_acc_lane == self.dp_last_lane {
                    self.dp_store_data = ((sum >> 16) & 0xFFFF_FFFF) as u32;
                    self.dp_store = true;
                    self.dp_store_mode = false;
                }
                self.dp_acc_lane += 1;
            }
            if ctx.dp_store_prev {
                self.dp_store = false;
                self.dp_acc = 0;
            }
        }
    }

    /// Shared multiply pipe transfer: one RTL always block of
    /// CpuV3FpuMulPipe. The stages move in reverse order so every stage reads
    /// its predecessor's pre-edge value; the operand mux gives the special
    /// path's SINCOS issue the tie, then multiply, then dot (the core never
    /// overlaps the owners). Reads only the pre-edge operands in `ctx`.
    fn tick_mul_pipe(&mut self, ctx: &TickContext) {
        let pipe_s1v = self.pipe_s1_valid;
        let pipe_s1a = self.pipe_s1_a;
        let pipe_s1b = self.pipe_s1_b;
        let pipe_s1tag = self.pipe_s1_tag;
        let pipe_s2v = self.pipe_s2_valid;
        let pipe_s2p = self.pipe_s2_prod;
        let pipe_s2tag = self.pipe_s2_tag;
        if ctx.abort {
            self.pipe_s1_valid = false;
            self.pipe_s2_valid = false;
            self.pipe_s3_valid = false;
        } else {
            self.pipe_s3_valid = pipe_s2v;
            if pipe_s2v {
                self.pipe_s3_prod = pipe_s2p;
                self.pipe_s3_tag = pipe_s2tag;
            }
            self.pipe_s2_valid = pipe_s1v;
            if pipe_s1v {
                self.pipe_s2_prod = pipe_s1a * pipe_s1b;
                self.pipe_s2_tag = pipe_s1tag;
            }
            // The operand mux: the special path's SINCOS issue wins the tie,
            // then multiply, then dot (never more than one active).
            let in_valid = if ctx.sf_mul_in_valid {
                true
            } else if self.mp_run || ctx.mp_load_now {
                ctx.mp_data_valid
            } else {
                ctx.dp_data_valid
            };
            self.pipe_s1_valid = in_valid;
            if in_valid {
                if ctx.sf_mul_in_valid {
                    self.pipe_s1_a = ctx.sf_mul_in_a;
                    self.pipe_s1_b = ctx.sf_mul_in_b;
                    self.pipe_s1_tag = ctx.sf_mul_in_tag;
                } else if self.mp_run || ctx.mp_load_now {
                    self.pipe_s1_a = ctx.rf_read_a as i32 as i64;
                    self.pipe_s1_b = ctx.rf_read_b as i32 as i64;
                    self.pipe_s1_tag = ctx.mp_waddr_prev;
                } else {
                    self.pipe_s1_a = ctx.rf_read_a as i32 as i64;
                    self.pipe_s1_b = ctx.rf_read_b as i32 as i64;
                    self.pipe_s1_tag = ctx.dp_lane_prev as u16 - 1;
                }
            }
        }
    }

    /// Special-path register updates: the single always block of
    /// CpuV3FpuSpecialPath. The snapshot in `input` is sampled before any path
    /// update, so the leaf sees the pre-edge state the RTL sees.
    fn tick_special_path(&mut self, input: &CpuV3FpuSpecialPathInputValue) {
        self.special_path.tick(input);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The architecture `encoding` module is the public source of truth for
    /// every FPU v2 field. This module keeps a private copy for the Verilog
    /// literals, so replay the whole 16-bit word space and require the two to
    /// agree exactly. A silent second encoding can never survive this test.
    #[test]
    fn architecture_encoding_locks_the_private_rtl_extractor() {
        use crate::{
            fpu_aux_fa, fpu_aux_x, fpu_fa, fpu_fb, fpu_fd, fpu_mode, fpu_scalar_subop_field,
            fpu_vector_len_field, fpu_vector_subop_field, FpuAuxKind, FpuOpcode, FpuVectorLength,
        };
        for word in 0..=u16::MAX {
            let major = (word >> 12) as u8;
            assert_eq!(encoding::opcode(word), major, "opcode {word:#06x}");
            assert_eq!(
                FpuOpcode::from_word0(word).is_some(),
                matches!(major, 0xc..=0xe),
                "fpu opcode set {word:#06x}"
            );
            assert_eq!(encoding::word0_fa(word), fpu_fa(word), "{word:#06x}");
            assert_eq!(encoding::word0_fb(word), fpu_fb(word), "{word:#06x}");
            assert_eq!(encoding::aux_fa(word), fpu_aux_fa(word), "{word:#06x}");
            assert_eq!(encoding::aux_x(word), fpu_aux_x(word), "{word:#06x}");
            assert_eq!(
                encoding::aux_kind(word),
                FpuAuxKind::from_field((word & 3) as u8) as u8,
                "{word:#06x}"
            );
            assert_eq!(encoding::word1_fd(word), fpu_fd(word), "{word:#06x}");
            assert_eq!(
                encoding::word1_len(word),
                fpu_vector_len_field(word),
                "{word:#06x}"
            );
            assert_eq!(
                encoding::vector_subop(word),
                fpu_vector_subop_field(word),
                "{word:#06x}"
            );
            assert_eq!(
                encoding::scalar_subop(word),
                fpu_scalar_subop_field(word),
                "{word:#06x}"
            );
            assert_eq!(encoding::word1_mode(word), fpu_mode(word), "{word:#06x}");
        }
        for len in 0..4u8 {
            assert_eq!(
                encoding::decoded_last_lane(len),
                FpuVectorLength::from_field(len).map_or(3, |len| len.lanes() - 1),
                "len {len}"
            );
        }
    }

    /// Three-stage mirror of the shared CpuV3FpuMulPipe used by the leaf-level
    /// SINCOS cycle test, since the pipeline itself lives in the unit top.
    #[derive(Default)]
    struct PipeStub {
        s1_valid: bool,
        s1_a: i64,
        s1_b: i64,
        s1_tag: u16,
        s2_valid: bool,
        s2_prod: i64,
        s2_tag: u16,
        s3_valid: bool,
        s3_prod: i64,
        s3_tag: u16,
    }

    impl PipeStub {
        fn step(&mut self, abort: bool, in_valid: bool, in_a: i64, in_b: i64, in_tag: u16) {
            if abort {
                self.s1_valid = false;
                self.s2_valid = false;
                self.s3_valid = false;
                return;
            }
            self.s3_valid = self.s2_valid;
            if self.s2_valid {
                self.s3_prod = self.s2_prod;
                self.s3_tag = self.s2_tag;
            }
            self.s2_valid = self.s1_valid;
            if self.s1_valid {
                self.s2_prod = self.s1_a * self.s1_b;
                self.s2_tag = self.s1_tag;
            }
            self.s1_valid = in_valid;
            if in_valid {
                self.s1_a = in_a;
                self.s1_b = in_b;
                self.s1_tag = in_tag;
            }
        }

        fn out(&self) -> (bool, u64, u64) {
            (self.s3_valid, self.s3_prod as u64, u64::from(self.s3_tag))
        }
    }

    /// Drives the private special-path cycle model through one SINCOS the same
    /// way the parent does (operand on the pre-T0 RF read, packed interval on
    /// the one-cycle RF read) with a local copy of the shared pipe feeding the
    /// T3 range product, and checks both writes against the frozen
    /// `lut::sincos_q16` reference (itself the two-term C0/C1 reducer). This is
    /// the leaf-level model/RTL agreement guard: the Verilog leaf testbench
    /// checks the RTL against an independent reference, and this test pins the
    /// Rust mirror and all three ISA modes to the same numbers.
    #[test]
    fn sincos_cycle_model_matches_reference() {
        let packed = lut::packed_sincos_lut();
        let fd = 8u16;
        let mut checked = 0u32;

        let mut check = |a: i32, mode: u16| {
            let word1 = (fd << 10) | (u16::from(encoding::SINCOS) << 4) | mode;
            let cycles = if matches!(mode, 1 | 2) { 7 } else { 8 };
            let mut state = CpuV3FpuSpecialPathState::default();
            let mut pipe = PipeStub::default();
            let mut read_a = a as u32;
            let mut writes: Vec<(u16, u32)> = Vec::new();
            let mut busy_count = 0u32;
            for cycle in 0..cycles {
                let (pipe_valid, pipe_product, pipe_tag) = pipe.out();
                let input = CpuV3FpuSpecialPathInputValue {
                    abort: false,
                    instr_complete: cycle == 0,
                    instr_opcode: u64::from(encoding::OPCODE_SCALAR),
                    word1_raw: u64::from(word1),
                    rf_read_a_data: u64::from(read_a),
                    rf_read_b_data: 0,
                    mul_out_valid: pipe_valid,
                    mul_out_product: pipe_product,
                    mul_out_tag: pipe_tag,
                };
                let out = state.comb(&input);
                if out.busy {
                    busy_count += 1;
                }
                if out.rf_write_enable {
                    writes.push((out.rf_write_address as u16, out.rf_write_data as u32));
                }
                pipe.step(
                    false,
                    out.mul_in_valid,
                    out.mul_in_a as i64,
                    out.mul_in_b as i64,
                    out.mul_in_tag as u16,
                );
                state.tick(&input);
                // The synchronous RF presents the previous cycle's address.
                let address = out.rf_read_a_address as usize;
                read_a = if (256..512).contains(&address) {
                    packed[address - 256]
                } else {
                    0
                };
            }
            let (sin, cos) = lut::sincos_q16(a);
            let expected = match mode {
                1 => vec![(fd, sin as u32)],
                2 => vec![(fd, cos as u32)],
                _ => vec![(fd, sin as u32), (fd + 1, cos as u32)],
            };
            assert_eq!(
                writes, expected,
                "sincos cycle model mismatch at a={a}, mode={mode}"
            );
            assert_eq!(
                busy_count, cycles,
                "sincos busy length mismatch at a={a}, mode={mode}"
            );
            checked += 1;
        };

        // [-2*pi, +2*pi] sampled densely, the exact quadrant endpoints, and
        // every power-of-two / full-range boundary.
        let limit = (2.0 * std::f64::consts::PI * 65536.0).round() as i32;
        let mut a = -limit;
        while a <= limit {
            check(a, 0);
            a += 37;
        }
        for a in [
            0,
            0x0001_0000,
            -0x0001_0000,
            102_944,  // pi/2
            -102_944, // -pi/2
            205_887,  // pi
            411_775,  // 2pi
            i32::MAX,
            i32::MIN,
        ] {
            check(a, 0);
            check(a, 1);
            check(a, 2);
        }
        let mut a = i32::MIN;
        while let Some(next) = a.checked_add(1_000_003) {
            check(a, 0);
            a = next;
        }
        assert!(checked > 1000, "expected a dense sweep, checked={checked}");
    }

    /// Abort asserted on the T3 range-product beat must clear busy
    /// combinationally, gate both writes and void the shared-pipe product; a
    /// following SINCOS then computes normally from the clean context.
    #[test]
    fn sincos_cycle_model_abort_cancels_without_writes() {
        let packed = lut::packed_sincos_lut();
        let fd = 8u16;
        let a = 0x0001_0000i32;
        let word1 = (fd << 10) | (u16::from(encoding::SINCOS) << 4);
        let mut state = CpuV3FpuSpecialPathState::default();
        let mut pipe = PipeStub::default();
        let mut read_a = a as u32;

        for cycle in 0..6 {
            let abort = cycle == 3;
            let (pipe_valid, pipe_product, pipe_tag) = pipe.out();
            let input = CpuV3FpuSpecialPathInputValue {
                abort,
                instr_complete: cycle == 0,
                instr_opcode: u64::from(encoding::OPCODE_SCALAR),
                word1_raw: u64::from(word1),
                rf_read_a_data: u64::from(read_a),
                rf_read_b_data: 0,
                mul_out_valid: pipe_valid,
                mul_out_product: pipe_product,
                mul_out_tag: pipe_tag,
            };
            let out = state.comb(&input);
            if cycle < 3 {
                assert!(out.busy, "SINCOS must be busy before abort");
            } else {
                assert!(!out.busy, "abort must clear busy at cycle {cycle}");
            }
            assert!(
                !out.rf_write_enable,
                "abort must not present a write at cycle {cycle}"
            );
            pipe.step(
                abort,
                out.mul_in_valid,
                out.mul_in_a as i64,
                out.mul_in_b as i64,
                out.mul_in_tag as u16,
            );
            state.tick(&input);
            let address = out.rf_read_a_address as usize;
            read_a = if (256..512).contains(&address) {
                packed[address - 256]
            } else {
                0
            };
        }

        // The next SINCOS must compute normally from a clean context.
        let (sin, cos) = lut::sincos_q16(a);
        let mut read_a = a as u32;
        let mut got: Vec<(u16, u32)> = Vec::new();
        for cycle in 0..8 {
            let (pipe_valid, pipe_product, pipe_tag) = pipe.out();
            let input = CpuV3FpuSpecialPathInputValue {
                abort: false,
                instr_complete: cycle == 0,
                instr_opcode: u64::from(encoding::OPCODE_SCALAR),
                word1_raw: u64::from(word1),
                rf_read_a_data: u64::from(read_a),
                rf_read_b_data: 0,
                mul_out_valid: pipe_valid,
                mul_out_product: pipe_product,
                mul_out_tag: pipe_tag,
            };
            let out = state.comb(&input);
            if out.rf_write_enable {
                got.push((out.rf_write_address as u16, out.rf_write_data as u32));
            }
            pipe.step(
                false,
                out.mul_in_valid,
                out.mul_in_a as i64,
                out.mul_in_b as i64,
                out.mul_in_tag as u16,
            );
            state.tick(&input);
            let address = out.rf_read_a_address as usize;
            read_a = if (256..512).contains(&address) {
                packed[address - 256]
            } else {
                0
            };
        }
        assert_eq!(
            got,
            vec![(fd, sin as u32), (fd + 1, cos as u32)],
            "post-abort SINCOS must be clean"
        );
    }

    /// Algebraic proof of the single-constant reducer: `C0*2^16 + C1 == K`, so
    /// the 18-bit phase `(a * K) >>> 32` equals the low 18 bits of the frozen
    /// full-width two-term reduction `h0 + ((((p0 + h1) << 16) + p1) >> 32)`
    /// for every input. This isolates the exact identity the RTL now implements
    /// through the shared 36x36 pipe and is independent of the LUT tables.
    #[test]
    fn sincos_k_reducer_matches_two_term_full_width() {
        const C0: i64 = 41_722;
        const C1: i64 = -31_890;

        fn full_width(a: i32) -> i32 {
            let a_hi = i64::from(a >> 16);
            let a_lo = i64::from(a & 0xFFFF);
            let p0 = a_lo * C0;
            let p1 = a_lo * C1;
            let h0 = a_hi * C0;
            let h1 = a_hi * C1;
            let t = h0 + ((((p0 + h1) << 16) + p1) >> 32);
            (t & 0x3_FFFF) as i32
        }

        fn k_reducer(a: i32) -> i32 {
            let product = i64::from(a) * SINCOS_K;
            ((product >> 32) & 0x3_FFFF) as i32
        }

        assert_eq!(C0 * (1 << 16) + C1, SINCOS_K, "K must be C0*2^16 + C1");

        let mut checked = 0u32;
        let mut verify = |a: i32| {
            assert_eq!(k_reducer(a), full_width(a), "K reducer mismatch at a={a}");
            checked += 1;
        };

        // Directed extremes and signed-boundary samples.
        for a in [
            0,
            1,
            -1,
            0x0000_FFFF,
            0x0001_0000,
            0x0001_0001,
            -0x0001_0000,
            -0x0001_0001,
            i32::MAX,
            i32::MIN,
            i32::MAX - 1,
            i32::MIN + 1,
        ] {
            verify(a);
        }

        // Dense [-2*pi, +2*pi].
        let limit = (2.0 * std::f64::consts::PI * 65536.0).round() as i32;
        let mut a = -limit;
        while a <= limit {
            verify(a);
            a += 37;
        }

        // Deterministic full-i32 sweep (same stride as the cycle-model test).
        let mut a = i32::MIN;
        while let Some(next) = a.checked_add(1_000_003) {
            verify(a);
            a = next;
        }
        assert!(checked > 1000, "expected a dense sweep, checked={checked}");
    }
}
