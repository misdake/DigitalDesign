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
    /// scalar and multiply paths act on are named here.
    pub(crate) const MUL: u8 = 0x02;
    pub(crate) const CMP: u8 = 0x0B;

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

pub(crate) mod lut;

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

pub struct CpuV3FpuRegisterRamState {
    memory: Box<[u32; 512]>,
    read_a_data: u32,
    read_b_data: u32,
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
        CpuV3FpuRegisterRamState {
            memory: Box::new([0; 512]),
            read_a_data: 0,
            read_b_data: 0,
        }
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
        // Both reads observe the pre-write contents (read-first semantics).
        state.read_a_data = state.memory[input.read_a_address as usize];
        state.read_b_data = state.memory[input.read_b_address as usize];
        if input.write_enable {
            state.memory[input.write_address as usize] = input.write_data as u32;
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
    pub mul_in_a: Wires<32>,
    pub mul_in_b: Wires<32>,
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
    pub in_a: Wires<32>,
    pub in_b: Wires<32>,
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
    pub mul_in_a: Wires<32>,
    pub mul_in_b: Wires<32>,
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
            rf: CpuV3FpuRegisterRamState {
                memory: Box::new([0; 512]),
                read_a_data: 0,
                read_b_data: 0,
            },
            scalar_path: CpuV3FpuScalarPathState::default(),
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
        CpuV3FpuOutputValue {
            busy: sp_w_wait != 0 || vp_busy || mp_busy || dp_busy,
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
            // MUL is owned by the multiply path.
            let load_now = instr_complete_prev
                && state.frontend.instr_opcode == encoding::OPCODE_SCALAR
                && subop != encoding::MUL;
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
        let read_a_address = if dp_load_now {
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
        let read_b_address = if dp_load_now {
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

        // Register-file updates last: reads sample the pre-write contents.
        let rf = &mut state.rf;
        rf.read_a_data = rf.memory[read_a_address];
        rf.read_b_data = rf.memory[read_b_address];
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
        } else {
            (
                sp.write_enable && !input.abort,
                sp.write_address as usize,
                sp.write_data,
            )
        };
        if we {
            rf.memory[wa] = wd;
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
    /// its predecessor's pre-edge value; the operand mux gives the multiply
    /// path the tie (never both). Reads only the pre-edge operands in `ctx`.
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
            // The operand mux: the multiply path wins the tie (never both).
            let in_valid = if self.mp_run || ctx.mp_load_now {
                ctx.mp_data_valid
            } else {
                ctx.dp_data_valid
            };
            self.pipe_s1_valid = in_valid;
            if in_valid {
                if self.mp_run || ctx.mp_load_now {
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
}
