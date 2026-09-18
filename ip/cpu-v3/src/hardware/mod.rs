//! Reusable CpuV3 revision 0.8 processor core with physical-memory and device ports.
//!
//! See [`../../docs/hardware-architecture.md`](../../docs/hardware-architecture.md) for the
//! current Stage 12 microarchitecture and fitted-cache boundary.

mod cache;
mod fetch;
pub use cache::*;
pub use fetch::*;

use digital_design_circuit::{CircuitWires, Wire, Wires};
use digital_design_hardware::{
    resources::components::{BsramBlocks, SsramBits},
    HardwareIdentity, Module, ModuleIo, TargetResourceRequest, VerilogDependency, VerilogIdentity,
};
use digital_design_hardware_gowin::DspMulS18;
use std::cmp::Ordering;

use crate::SignalEvent;

pub const CPU_V3_FAULT_INVALID_INSTRUCTION: u8 = 1;
pub const CPU_V3_FAULT_INSTRUCTION_MEMORY: u8 = 3;
pub const CPU_V3_FAULT_DATA_MEMORY: u8 = 4;

#[derive(Clone, ModuleIo)]
pub struct CpuV3CoreInput {
    pub reset: Wire,
    pub hold: Wire,
    pub instruction_request_ready: Wire,
    pub instruction_response_valid: Wire,
    pub instruction_data: Wires<16>,
    pub instruction_error: Wire,
    pub data_request_ready: Wire,
    pub data_response_valid: Wire,
    pub data_read_data: Wires<16>,
    pub data_error: Wire,
    pub device_read_data: Wires<16>,
}

#[derive(Clone, ModuleIo)]
pub struct CpuV3CoreOutput {
    pub instruction_request_valid: Wire,
    pub instruction_address: Wires<32>,
    pub instruction_response_ready: Wire,
    pub data_request_valid: Wire,
    pub data_write: Wire,
    pub data_address: Wires<32>,
    pub data_write_data: Wires<16>,
    pub data_response_ready: Wire,
    pub device_index: Wires<3>,
    pub device_channel: Wires<4>,
    pub device_read_enable: Wire,
    pub device_write_enable: Wire,
    pub device_write_data: Wires<16>,
    pub halted: Wire,
    pub halt_signal: Wires<16>,
    pub fault: Wire,
    pub fault_code: Wires<8>,
    pub fault_pc: Wires<16>,
    pub pc: Wires<16>,
    pub code_segment: Wires<16>,
    pub data_segment: Wires<16>,
    pub retired_words: Wires<32>,
}

pub struct CpuV3Core;

/// SSRAM physical bits for the scalar register file: 16 words x 16 bits with
/// two asynchronous read ports, which Gowin builds from two copies of four
/// RAM16X4 cells.
const CPU_V3_GPR_RAM16S: usize = 2 * 4;
const CPU_V3_GPR_PHYSICAL_BITS: usize = CPU_V3_GPR_RAM16S * 64;

#[derive(Clone, ModuleIo)]
pub struct CpuV3GprRamInput {
    pub write_enable: Wire,
    pub write_address: Wires<4>,
    pub write_data: Wires<16>,
    pub read_a_address: Wires<4>,
    pub read_b_address: Wires<4>,
}

#[derive(Clone, ModuleIo)]
pub struct CpuV3GprRamOutput {
    pub read_a_data: Wires<16>,
    pub read_b_data: Wires<16>,
}

/// Synchronous-write, dual-asynchronous-read distributed RAM holding the
/// sixteen scalar registers.
pub struct CpuV3GprRam;

impl HardwareIdentity for CpuV3GprRam {
    const TARGET_RESOURCE_LEAF: bool = true;

    fn verilog_identity() -> VerilogIdentity {
        VerilogIdentity::new("CpuV3GprRam").namespace(["components", "cpu", "cpu_v3"])
    }
}

impl Module for CpuV3GprRam {
    type Input = CpuV3GprRamInput;
    type Output = CpuV3GprRamOutput;
    type EmuState = [u16; 16];

    const USES_MAIN_CLOCK: bool = true;

    fn target_resources() -> Vec<TargetResourceRequest> {
        vec![TargetResourceRequest::new(SsramBits::new(
            CPU_V3_GPR_PHYSICAL_BITS as u64,
        ))]
    }

    fn create_emu(_input: &Self::Input, _output: &Self::Output) -> Self::EmuState {
        [0; 16]
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
            &CpuV3GprRamOutputValue {
                read_a_data: u64::from(state[input.read_a_address as usize]),
                read_b_data: u64::from(state[input.read_b_address as usize]),
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
            state[input.write_address as usize] = input.write_data as u16;
        }
    }

    fn verilog_source() -> Option<String> {
        Some(include_str!("cpu_v3_gpr_ram.v").to_string())
    }

    fn verilog_testbench() -> Option<String> {
        Some(include_str!("cpu_v3_gpr_ram_tb.v").to_string())
    }
}

#[derive(Clone, ModuleIo)]
pub struct CpuV3FpuV2RegisterRamInput {
    pub write_enable: Wire,
    pub write_address: Wires<9>,
    pub write_data: Wires<32>,
    pub read_a_address: Wires<9>,
    pub read_b_address: Wires<9>,
}

#[derive(Clone, ModuleIo)]
pub struct CpuV3FpuV2RegisterRamOutput {
    pub read_a_data: Wires<32>,
    pub read_b_data: Wires<32>,
}

/// FPU v2 register file: two mirrored 512x32 BSRAMs (SDPB) giving one
/// broadcast write port plus two independent synchronous read ports.
/// Physical addresses 0..63 alias architectural F0..F63 (the parent forces
/// the top three address bits of architectural accesses to zero); 64..511
/// are the hidden LUT region for RCP/RSQRT/SINCOS. Same-cycle write/read on
/// one address returns the old word on both read ports.
pub struct CpuV3FpuV2RegisterRam;

impl HardwareIdentity for CpuV3FpuV2RegisterRam {
    const TARGET_RESOURCE_LEAF: bool = true;

    fn verilog_identity() -> VerilogIdentity {
        VerilogIdentity::new("CpuV3FpuV2RegisterRam").namespace(["components", "cpu", "cpu_v3"])
    }
}

pub struct CpuV3FpuV2RegisterRamState {
    memory: Box<[u32; 512]>,
    read_a_data: u32,
    read_b_data: u32,
}

impl Module for CpuV3FpuV2RegisterRam {
    type Input = CpuV3FpuV2RegisterRamInput;
    type Output = CpuV3FpuV2RegisterRamOutput;
    type EmuState = CpuV3FpuV2RegisterRamState;

    const USES_MAIN_CLOCK: bool = true;

    fn target_resources() -> Vec<TargetResourceRequest> {
        vec![TargetResourceRequest::new(BsramBlocks::new(2))]
    }

    fn create_emu(_input: &Self::Input, _output: &Self::Output) -> Self::EmuState {
        CpuV3FpuV2RegisterRamState {
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
            &CpuV3FpuV2RegisterRamOutputValue {
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
        Some(include_str!("cpu_v3_fpu_v2_register_ram.v").to_string())
    }

    fn verilog_testbench() -> Option<String> {
        Some(include_str!("cpu_v3_fpu_v2_register_ram_tb.v").to_string())
    }
}

#[derive(Clone, ModuleIo)]
pub struct CpuV3FpuV2FrontendInput {
    pub word_valid: Wire,
    pub word: Wires<16>,
    pub abort: Wire,
}

#[derive(Clone, ModuleIo)]
pub struct CpuV3FpuV2FrontendOutput {
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
pub struct CpuV3FpuV2Frontend;

impl HardwareIdentity for CpuV3FpuV2Frontend {
    const TARGET_RESOURCE_LEAF: bool = true;

    fn verilog_identity() -> VerilogIdentity {
        VerilogIdentity::new("CpuV3FpuV2Frontend").namespace(["components", "cpu", "cpu_v3"])
    }
}

#[derive(Default)]
pub struct CpuV3FpuV2FrontendState {
    waiting_word1: bool,
    word0_raw: u16,
    word1_raw: u16,
    instr_opcode: u8,
    instr_complete: bool,
}

impl Module for CpuV3FpuV2Frontend {
    type Input = CpuV3FpuV2FrontendInput;
    type Output = CpuV3FpuV2FrontendOutput;
    type EmuState = CpuV3FpuV2FrontendState;

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
        let read_a = if word >> 12 == 0xE {
            (word >> 2) & 0x3F
        } else {
            (word >> 6) & 0x3F
        };
        output.drive(
            circuit,
            &CpuV3FpuV2FrontendOutputValue {
                read_valid,
                rf_read_a_address: u64::from(read_a),
                rf_read_b_address: u64::from(word & 0x3F),
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
            state.instr_opcode = (word >> 12) as u8;
            state.waiting_word1 = true;
        } else if accept_word1 || discard_word0 {
            state.waiting_word1 = false;
        }
        if accept_word1 {
            state.word1_raw = word;
        }
    }

    fn verilog_source() -> Option<String> {
        Some(include_str!("cpu_v3_fpu_v2_frontend.v").to_string())
    }

    fn verilog_testbench() -> Option<String> {
        Some(include_str!("cpu_v3_fpu_v2_frontend_tb.v").to_string())
    }
}

#[derive(Clone, ModuleIo)]
pub struct CpuV3FpuV2ScalarAluInput {
    pub a: Wires<32>,
    pub b: Wires<32>,
    pub op: Wires<4>,
}

#[derive(Clone, ModuleIo)]
pub struct CpuV3FpuV2ScalarAluOutput {
    pub result: Wires<32>,
    pub flag_lt: Wire,
    pub flag_eq: Wire,
    pub flag_gt: Wire,
}

/// FPU v2 scalar Q16.16 ALU: purely combinational, all arithmetic in fabric
/// LUTs (syn_dspstyle="logic"; DSP absorption of wide adds is a measured
/// Gowin pitfall). Wrap overflow policy, round-half-up ROUND; flags are only
/// specified for op == CMP.
pub struct CpuV3FpuV2ScalarAlu;

impl HardwareIdentity for CpuV3FpuV2ScalarAlu {
    const TARGET_RESOURCE_LEAF: bool = true;

    fn verilog_identity() -> VerilogIdentity {
        VerilogIdentity::new("CpuV3FpuV2ScalarAlu").namespace(["components", "cpu", "cpu_v3"])
    }
}

impl Module for CpuV3FpuV2ScalarAlu {
    type Input = CpuV3FpuV2ScalarAluInput;
    type Output = CpuV3FpuV2ScalarAluOutput;
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
            &CpuV3FpuV2ScalarAluOutputValue {
                result: u64::from(result),
                flag_lt: sa < sb,
                flag_eq: sa == sb,
                flag_gt: sa > sb,
            },
        );
    }

    fn verilog_source() -> Option<String> {
        Some(include_str!("cpu_v3_fpu_v2_scalar_alu.v").to_string())
    }

    fn verilog_testbench() -> Option<String> {
        Some(include_str!("cpu_v3_fpu_v2_scalar_alu_tb.v").to_string())
    }
}

#[derive(Clone, ModuleIo)]
pub struct CpuV3FpuV2ScalarPathInput {
    pub abort: Wire,
    pub instr_complete: Wire,
    pub instr_opcode: Wires<4>,
    pub word1_raw: Wires<16>,
    pub rf_read_a_data: Wires<32>,
    pub rf_read_b_data: Wires<32>,
}

#[derive(Clone, ModuleIo)]
pub struct CpuV3FpuV2ScalarPathOutput {
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
pub struct CpuV3FpuV2ScalarPath;

impl HardwareIdentity for CpuV3FpuV2ScalarPath {
    const TARGET_RESOURCE_LEAF: bool = false;

    fn verilog_identity() -> VerilogIdentity {
        VerilogIdentity::new("CpuV3FpuV2ScalarPath").namespace(["components", "cpu", "cpu_v3"])
    }
}

#[derive(Default)]
pub struct CpuV3FpuV2ScalarPathState {
    write_enable: bool,
    write_address: u16,
    write_data: u32,
    flag_lt: bool,
    flag_eq: bool,
    flag_gt: bool,
    w_count: u8,
    x_count: u8,
}

impl CpuV3FpuV2ScalarPathState {
    /// Mirrors CpuV3FpuV2ScalarAlu: wrap-only Q16.16 ops.
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

impl Module for CpuV3FpuV2ScalarPath {
    type Input = CpuV3FpuV2ScalarPathInput;
    type Output = CpuV3FpuV2ScalarPathOutput;
    type EmuState = CpuV3FpuV2ScalarPathState;

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
        let load_now = input.instr_complete && input.instr_opcode == 0xD && !input.abort;
        let w_wait = if load_now { 2 } else { state.w_count };
        let x_wait = if load_now { 2 } else { state.x_count };
        output.drive(
            circuit,
            &CpuV3FpuV2ScalarPathOutputValue {
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
        let subop = ((word1 >> 4) & 0x3F) as u8;
        let fd = (word1 >> 10) & 0x3F;
        let is_cmp = subop == 0x0B;
        let load_now = input.instr_complete && input.instr_opcode == 0xD;
        state.write_enable = load_now && !is_cmp;
        if load_now {
            let (result, lt, eq, gt) = Self::EmuState::alu(
                input.rf_read_a_data as u32,
                input.rf_read_b_data as u32,
                subop,
            );
            state.write_address = fd;
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
        Some(include_str!("cpu_v3_fpu_v2_scalar_path.v").to_string())
    }

    fn verilog_dependencies() -> Vec<VerilogDependency> {
        vec![
            VerilogDependency::new::<CpuV3FpuV2ScalarAlu>("scalar_alu"),
            VerilogDependency::new::<CpuV3FpuV2Frontend>("frontend"),
            VerilogDependency::new::<CpuV3FpuV2RegisterRam>("rf"),
        ]
    }

    fn verilog_testbench() -> Option<String> {
        Some(include_str!("cpu_v3_fpu_v2_scalar_path_tb.v").to_string())
    }
}

#[derive(Clone, ModuleIo)]
pub struct CpuV3FpuV2Input {
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
pub struct CpuV3FpuV2Output {
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
pub struct CpuV3FpuV2;

impl HardwareIdentity for CpuV3FpuV2 {
    const TARGET_RESOURCE_LEAF: bool = false;

    fn verilog_identity() -> VerilogIdentity {
        VerilogIdentity::new("CpuV3FpuV2").namespace(["components", "cpu", "cpu_v3"])
    }
}

/// Compositional emu state: the three leaf states plus the one-beat
/// operand-address hold register.
pub struct CpuV3FpuV2State {
    frontend: CpuV3FpuV2FrontendState,
    rf: CpuV3FpuV2RegisterRamState,
    scalar_path: CpuV3FpuV2ScalarPathState,
    held_read_a_address: u16,
    held_read_b_address: u16,
}

impl Default for CpuV3FpuV2State {
    fn default() -> Self {
        CpuV3FpuV2State {
            frontend: CpuV3FpuV2FrontendState::default(),
            rf: CpuV3FpuV2RegisterRamState {
                memory: Box::new([0; 512]),
                read_a_data: 0,
                read_b_data: 0,
            },
            scalar_path: CpuV3FpuV2ScalarPathState::default(),
            held_read_a_address: 0,
            held_read_b_address: 0,
        }
    }
}

impl CpuV3FpuV2State {
    /// Combinational outputs of the unit top (registered leaf outputs and the
    /// busy/complete status); register updates live in `clock_emu`.
    fn step(&mut self, input: &CpuV3FpuV2InputValue) -> CpuV3FpuV2OutputValue {
        let sp = &self.scalar_path;
        let sp_load_now =
            self.frontend.instr_complete && self.frontend.instr_opcode == 0xD && !input.abort;
        let sp_w_wait = if sp_load_now { 2 } else { sp.w_count };
        CpuV3FpuV2OutputValue {
            busy: sp_w_wait != 0,
            flag_lt: self.scalar_path.flag_lt,
            flag_eq: self.scalar_path.flag_eq,
            flag_gt: self.scalar_path.flag_gt,
            instr_complete: self.frontend.instr_complete,
            ext_read_data: u64::from(self.rf.read_b_data),
        }
    }
}

impl Module for CpuV3FpuV2 {
    type Input = CpuV3FpuV2Input;
    type Output = CpuV3FpuV2Output;
    type EmuState = CpuV3FpuV2State;

    const USES_MAIN_CLOCK: bool = true;

    fn target_resources() -> Vec<TargetResourceRequest> {
        // The register file claims two 18-Kbit BSRAM blocks.
        vec![TargetResourceRequest::new(BsramBlocks::new(2))]
    }

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
        output.drive(circuit, &state.step(&input));
    }

    fn clock_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        input: &Self::Input,
        _output: &Self::Output,
    ) {
        let input = input.sample(circuit);
        let word = input.word as u16;

        // Front-end register updates (identical to the leaf clock_emu).
        let accept_word0 = input.word_valid && !state.frontend.waiting_word1;
        let accept_word1 = input.word_valid && state.frontend.waiting_word1 && !input.abort;
        let discard_word0 = input.abort && state.frontend.waiting_word1;
        state.frontend.instr_complete = accept_word1;
        if accept_word0 {
            state.frontend.word0_raw = word;
            state.frontend.instr_opcode = (word >> 12) as u8;
            state.frontend.waiting_word1 = true;
            // The wrapper captures the read addresses on the word0 beat.
            state.held_read_a_address = if word >> 12 == 0xE {
                (word >> 2) & 0x3F
            } else {
                (word >> 6) & 0x3F
            };
            state.held_read_b_address = word & 0x3F;
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
            let subop = ((word1 >> 4) & 0x3F) as u8;
            let fd = (word1 >> 10) & 0x3F;
            let is_cmp = subop == 0x0B;
            let load_now = state.frontend.instr_complete && state.frontend.instr_opcode == 0xD;
            // read-first: the RF read registers still hold T0 operands here.
            sp.write_enable = load_now && !is_cmp;
            if load_now {
                let (result, lt, eq, gt) = CpuV3FpuV2ScalarPathState::alu(
                    state.rf.read_a_data,
                    state.rf.read_b_data,
                    subop,
                );
                sp.write_address = fd;
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

        // Register-file updates last: reads sample the pre-write contents.
        let rf = &mut state.rf;
        let sp = &state.scalar_path;
        let read_a_address = state.held_read_a_address as usize;
        let read_b_address = if input.ext_access {
            input.ext_read_address as usize
        } else {
            state.held_read_b_address as usize
        };
        rf.read_a_data = rf.memory[read_a_address];
        rf.read_b_data = rf.memory[read_b_address];
        let (we, wa, wd) = if input.ext_access {
            (
                input.ext_write_enable,
                input.ext_write_address as usize,
                input.ext_write_data as u32,
            )
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

    fn verilog_source() -> Option<String> {
        Some(include_str!("cpu_v3_fpu_v2.v").to_string())
    }

    fn verilog_dependencies() -> Vec<VerilogDependency> {
        vec![
            VerilogDependency::new::<CpuV3FpuV2Frontend>("frontend"),
            VerilogDependency::new::<CpuV3FpuV2RegisterRam>("rf"),
            VerilogDependency::new::<CpuV3FpuV2ScalarPath>("scalar_path"),
        ]
    }

    fn verilog_testbench() -> Option<String> {
        Some(include_str!("cpu_v3_fpu_v2_tb.v").to_string())
    }
}

impl HardwareIdentity for CpuV3Core {
    const TARGET_RESOURCE_LEAF: bool = false;

    fn verilog_identity() -> VerilogIdentity {
        VerilogIdentity::new("CpuV3Core").namespace(["components", "cpu", "cpu_v3"])
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Phase {
    FetchRequest,
    FetchResponse,
    Execute,
    DataRequest,
    DataResponse,
    AsyncStoreWait,
    MultiplyWait,
    MultiplyCommit,
    ResetClear,
    Halted,
    Fault,
}

#[derive(Clone, Copy)]
struct Prefix {
    address: u16,
    payload: u16,
}

#[derive(Clone, Copy, Default)]
struct PendingData {
    write: bool,
    address: u32,
    write_data: u16,
    destination: u8,
    retire_words: u8,
    fault_pc: u16,
}

#[derive(Clone, Copy, Default)]
struct AsyncStore {
    valid: bool,
    issued: bool,
    address: u32,
    write_data: u16,
    fault_pc: u16,
}

pub struct CpuV3CoreState {
    registers: [u16; 16],
    gpr_write_enable: bool,
    gpr_write_address: u8,
    gpr_write_data: u16,
    pc: u16,
    code_segment: u16,
    data_segment: u16,
    prefix: Option<Prefix>,
    /// Transient result of the last CMP-class instruction; mirrors the
    /// architectural `CpuV3Sim::pending_test` (consumed by conditional
    /// branches, expired by any other retired non-prefix instruction).
    pending_test: Option<Ordering>,
    phase: Phase,
    instruction: u16,
    instruction_pc: u16,
    pending_data: PendingData,
    async_store: AsyncStore,
    multiply_destination: u8,
    multiply_product: u32,
    multiply_shift: u8,
    multiply_retire_words: u8,
    /// Reset walks the scalar register file back to zero one word per cycle.
    clear_index: u8,
    retired_words: u32,
    fault_code: u8,
    fault_pc: u16,
    /// The architectural halt value, latched at the HALT retire edge like a
    /// register-read (mirrors the RTL's registered `halt_signal`).
    halt_signal: u16,
    /// Model-only SIGNAL event: set when a nonzero `SIGNAL` retires and held
    /// until the next executed instruction or `take_signal_event`. The RTL
    /// exposes no port for it and retires nonzero types as a NOP.
    signal_event: Option<SignalEvent>,
}

impl Default for CpuV3CoreState {
    fn default() -> Self {
        Self {
            registers: [0; 16],
            gpr_write_enable: false,
            gpr_write_address: 0,
            gpr_write_data: 0,
            pc: 0,
            code_segment: 0,
            data_segment: 0,
            prefix: None,
            pending_test: None,
            phase: Phase::FetchRequest,
            instruction: 0,
            instruction_pc: 0,
            pending_data: PendingData::default(),
            async_store: AsyncStore::default(),
            multiply_destination: 0,
            multiply_product: 0,
            multiply_shift: 0,
            multiply_retire_words: 0,
            clear_index: 0,
            retired_words: 0,
            fault_code: 0,
            fault_pc: 0,
            halt_signal: 0,
            signal_event: None,
        }
    }
}

impl CpuV3CoreState {
    fn fault(&mut self, code: u8, pc: u16) {
        self.fault_code = code;
        self.fault_pc = pc;
        self.phase = Phase::Fault;
    }

    fn retire(&mut self, words: u8) {
        self.retired_words = self.retired_words.wrapping_add(u32::from(words));
        self.phase = Phase::FetchRequest;
    }

    /// The model-only event of the most recently retired nonzero `SIGNAL`.
    /// Cleared by the next executed instruction or by `take_signal_event`.
    pub fn signal_event(&self) -> Option<SignalEvent> {
        self.signal_event
    }

    /// Reads and clears the pending model-only SIGNAL event.
    pub fn take_signal_event(&mut self) -> Option<SignalEvent> {
        self.signal_event.take()
    }

    /// Stages a write to the synchronous-write GPR RAM. The value lands one
    /// cycle later, matching the RTL's `gpr_write_enable` register + GPR RAM
    /// synchronous write port.
    fn write_gpr(&mut self, destination: u8, value: u16) {
        self.gpr_write_enable = true;
        self.gpr_write_address = destination;
        self.gpr_write_data = value;
    }

    fn execute(&mut self, device_read_data: u16) {
        let instruction = self.instruction;
        let opcode = instruction >> 12;
        // The model-only SIGNAL event lives for one executed instruction.
        self.signal_event = None;
        if opcode == 0xf {
            if self.prefix.is_some() {
                self.retired_words = self.retired_words.wrapping_add(1);
            }
            self.prefix = Some(Prefix {
                address: self.instruction_pc,
                payload: instruction & 0x0fff,
            });
            self.phase = Phase::FetchRequest;
            return;
        }

        let prefix = self.prefix.take();
        let consumes_prefix = is_prefix_consumer(instruction);
        // Every retired non-prefix instruction expires the pending test;
        // CMP-class instructions set it again below and conditional
        // branches consume the taken value.
        let pending = self.pending_test.take();
        if prefix.is_some() && !consumes_prefix {
            self.retired_words = self.retired_words.wrapping_add(1);
        }
        let retire_words = if prefix.is_some() && consumes_prefix {
            2
        } else {
            1
        };
        let fault_pc = if consumes_prefix {
            prefix.map_or(self.instruction_pc, |value| value.address)
        } else {
            self.instruction_pc
        };

        let dst = field(instruction, 8);
        let lhs = field(instruction, 4);
        let rhs = field(instruction, 0);
        match opcode {
            0 | 1 | 3..=5 => {
                let left = self.registers[usize::from(lhs)];
                let right = self.registers[usize::from(rhs)];
                self.write_gpr(
                    dst,
                    match opcode {
                        0 => left.wrapping_add(right),
                        1 => left.wrapping_sub(right),
                        3 => left & right,
                        4 => left | right,
                        _ => left ^ right,
                    },
                );
                self.retire(retire_words);
            }
            2 => {
                let function = dst;
                let old = self.registers[usize::from(lhs)];
                match function {
                    // Destructive shifts: register count (0..=2) or immediate
                    // (4..=6).
                    0..=2 | 4..=6 => {
                        let amount = if function < 4 {
                            self.registers[usize::from(rhs)] & 15
                        } else {
                            u16::from(rhs)
                        };
                        let result = match function & 3 {
                            0 => old.wrapping_shl(u32::from(amount)),
                            1 => old.wrapping_shr(u32::from(amount)),
                            _ => ((old as i16) >> u32::from(amount)) as u16,
                        };
                        self.write_gpr(lhs, result);
                        self.retire(retire_words);
                    }
                    // MUL0/MUL8/MUL16 and MULI share the single integer DSP.
                    8..=10 | 12 => {
                        let shift = match function {
                            9 => 8,
                            10 => 16,
                            _ => 0,
                        };
                        // MULI sources the unsigned immediate bit pattern.
                        let right = if function == 12 {
                            immediate4(instruction, prefix, false)
                        } else {
                            self.registers[usize::from(rhs)]
                        };
                        self.begin_multiply(lhs, old, right, shift, retire_words);
                    }
                    _ => self.fault(CPU_V3_FAULT_INVALID_INSTRUCTION, fault_pc),
                }
            }
            6 => self.execute_extended(instruction, retire_words, fault_pc),
            7 => {
                if dst & 8 == 0 {
                    self.write_gpr(rhs, device_read_data);
                }
                self.retire(retire_words);
            }
            8 | 9 => {
                let offset = immediate4(instruction, prefix, true);
                let logical = self.registers[usize::from(lhs)].wrapping_add(offset);
                let pending = PendingData {
                    write: opcode == 9,
                    address: physical_address(self.data_segment, logical),
                    write_data: self.registers[usize::from(dst)],
                    destination: dst,
                    retire_words,
                    fault_pc,
                };
                if opcode == 9 && !self.async_store.valid {
                    self.async_store = AsyncStore {
                        valid: true,
                        issued: false,
                        address: pending.address,
                        write_data: pending.write_data,
                        fault_pc: pending.fault_pc,
                    };
                    self.retire(retire_words);
                } else {
                    self.pending_data = pending;
                    self.phase = if self.async_store.valid {
                        Phase::AsyncStoreWait
                    } else {
                        Phase::DataRequest
                    };
                }
            }
            10 => self.execute_immediate(instruction, prefix, retire_words, fault_pc),
            11 => {
                let function = dst;
                let offset = prefix.map_or_else(
                    || sign_extend(instruction & 0xff, 8),
                    |value| ((value.payload & 0xff) << 8) | (instruction & 0xff),
                );
                match function {
                    // Conditional branches and conditional moves consume the
                    // pending test result, whether or not the condition holds.
                    0..=5 | 8..=13 => {
                        let Some(test) = pending else {
                            self.fault(CPU_V3_FAULT_INVALID_INSTRUCTION, fault_pc);
                            return;
                        };
                        let taken = match function & 7 {
                            0 => test == Ordering::Equal,
                            1 => test != Ordering::Equal,
                            2 => test == Ordering::Less,
                            3 => test != Ordering::Less,
                            4 => test == Ordering::Greater,
                            _ => test != Ordering::Greater,
                        };
                        if function <= 5 {
                            if taken {
                                self.pc = self.pc.wrapping_add(offset);
                            }
                        } else if taken {
                            // MOVcc rd, rs
                            self.write_gpr(lhs, self.registers[usize::from(rhs)]);
                        }
                    }
                    // JREL: unconditional relative jump, no link.
                    6 => self.pc = self.pc.wrapping_add(offset),
                    // JALREL: link the fall-through address into r14.
                    7 => {
                        let next = self.pc;
                        self.pc = next.wrapping_add(offset);
                        self.write_gpr(14, next);
                    }
                    // JREG: canonical `B E 0 target`.
                    14 if lhs == 0 => self.pc = self.registers[usize::from(rhs)],
                    // JALR: canonical `B F E target`, link fixed to r14.
                    15 if lhs == 14 => {
                        let target = self.registers[usize::from(rhs)];
                        self.write_gpr(14, self.pc);
                        self.pc = target;
                    }
                    _ => {
                        self.fault(CPU_V3_FAULT_INVALID_INSTRUCTION, fault_pc);
                        return;
                    }
                }
                self.retire(retire_words);
            }
            _ => self.fault(CPU_V3_FAULT_INVALID_INSTRUCTION, fault_pc),
        }
    }

    fn begin_multiply(
        &mut self,
        destination: u8,
        left: u16,
        right: u16,
        shift: u8,
        retire_words: u8,
    ) {
        self.multiply_destination = destination;
        // The DSP sees zero-extended inputs, so this is the full unsigned
        // 32-bit product; the window is selected at commit.
        self.multiply_product = u32::from(left) * u32::from(right);
        self.multiply_shift = shift;
        self.multiply_retire_words = retire_words;
        self.phase = Phase::MultiplyWait;
    }

    fn execute_immediate(
        &mut self,
        instruction: u16,
        prefix: Option<Prefix>,
        retire_words: u8,
        fault_pc: u16,
    ) {
        let function = field(instruction, 8);
        let dst = field(instruction, 4);
        let old = self.registers[usize::from(dst)];
        let signed = immediate4(instruction, prefix, true);
        let unsigned = immediate4(instruction, prefix, false);
        match function {
            // CMPSI/CMPUI set the pending test result and write no register.
            12 => {
                self.pending_test = Some((old as i16).cmp(&(signed as i16)));
                self.retire(retire_words);
                return;
            }
            13 => {
                self.pending_test = Some(old.cmp(&unsigned));
                self.retire(retire_words);
                return;
            }
            _ => {}
        }
        let result = match function {
            // ADDI/SUBI read the unprefixed immediate as an unsigned u4; the
            // prefixed form adds/subtracts the full 16-bit pattern.
            0 => old.wrapping_add(unsigned),
            1 => old.wrapping_sub(unsigned),
            2 if prefix.is_some() => unsigned,
            2 => sign_extend(instruction & 15, 4),
            3 => unsigned,
            4 => old & unsigned,
            5 => old | unsigned,
            6 => old ^ unsigned,
            // LDC/ADDC index the shared constant table; a pending prefix
            // expires unused (these never consume it).
            7 => crate::CONSTANT_TABLE[usize::from(instruction & 15)],
            8 => u16::from(old == signed),
            9 => u16::from((old as i16) < (signed as i16)),
            10 => u16::from(old < unsigned),
            11 => old.wrapping_add(crate::CONSTANT_TABLE[usize::from(instruction & 15)]),
            _ => {
                self.fault(CPU_V3_FAULT_INVALID_INSTRUCTION, fault_pc);
                return;
            }
        };
        self.write_gpr(dst, result);
        self.retire(retire_words);
    }

    fn execute_extended(&mut self, instruction: u16, retire_words: u8, fault_pc: u16) {
        let function = field(instruction, 8);
        let dst = field(instruction, 4);
        let src = field(instruction, 0);
        match function {
            0 => self.write_gpr(dst, self.registers[usize::from(src)]),
            1 => self.write_gpr(dst, !self.registers[usize::from(src)]),
            2 => self.write_gpr(dst, self.registers[usize::from(src)].wrapping_neg()),
            3 => self.write_gpr(dst, sign_extend(self.registers[usize::from(src)] & 0xff, 8)),
            4 => self.write_gpr(dst, self.registers[usize::from(src)].leading_zeros() as u16),
            5 => self.write_gpr(dst, self.registers[usize::from(src)].count_ones() as u16),
            6 => self.write_gpr(
                dst,
                u16::from(self.registers[usize::from(dst)] == self.registers[usize::from(src)]),
            ),
            8 => self.write_gpr(
                dst,
                u16::from(
                    (self.registers[usize::from(dst)] as i16)
                        < (self.registers[usize::from(src)] as i16),
                ),
            ),
            9 => self.write_gpr(
                dst,
                u16::from(self.registers[usize::from(dst)] < self.registers[usize::from(src)]),
            ),
            10 => {
                self.pending_test = Some(
                    (self.registers[usize::from(dst)] as i16)
                        .cmp(&(self.registers[usize::from(src)] as i16)),
                )
            }
            11 => {
                self.pending_test =
                    Some(self.registers[usize::from(dst)].cmp(&self.registers[usize::from(src)]))
            }
            // SIGNAL rs, type4: type 0 halts and latches rs at the retire
            // edge, like a register-read; nonzero types retire as a NOP and
            // record a model-only event (the RTL exposes no port for it).
            12 if src == 0 => {
                self.halt_signal = self.registers[usize::from(dst)];
                self.retired_words = self.retired_words.wrapping_add(u32::from(retire_words));
                self.phase = Phase::Halted;
                return;
            }
            12 => {
                self.signal_event = Some(SignalEvent {
                    signal_type: src,
                    value: self.registers[usize::from(dst)],
                });
            }
            13 => {
                let value = match src {
                    0 => self.code_segment,
                    1 => self.data_segment,
                    _ => {
                        self.fault(CPU_V3_FAULT_INVALID_INSTRUCTION, fault_pc);
                        return;
                    }
                };
                self.write_gpr(dst, value);
            }
            14 if dst == 1 => self.data_segment = self.registers[usize::from(src)],
            15 => {
                self.code_segment = self.registers[usize::from(dst)];
                self.pc = self.registers[usize::from(src)];
            }
            _ => {
                self.fault(CPU_V3_FAULT_INVALID_INSTRUCTION, fault_pc);
                return;
            }
        }
        self.retire(retire_words);
    }

    fn execute_pipelineable(&self) -> bool {
        if self.phase != Phase::Execute {
            return false;
        }
        let opcode = field(self.instruction, 12);
        let function = field(self.instruction, 8);
        let a = field(self.instruction, 4);
        let b = field(self.instruction, 0);
        match opcode {
            0 | 1 | 3..=5 | 15 => true,
            // Major 2: the single-cycle shifts; multiplies go through the
            // blocking DSP states and reserved functions fault.
            2 => function <= 2 || (4..=6).contains(&function),
            // Major 6: MOV..SEQ, SLT..CMPU, non-halting SIGNAL, valid MFSR,
            // and MTSR DSEG retire in one cycle.
            6 => {
                function <= 6
                    || (8..=11).contains(&function)
                    || (function == 12 && b != 0)
                    || (function == 13 && b <= 1)
                    || (function == 14 && a == 1)
            }
            9 => !self.async_store.valid,
            10 => function != 14 && function != 15,
            _ => false,
        }
    }
}

impl Module for CpuV3Core {
    type Input = CpuV3CoreInput;
    type Output = CpuV3CoreOutput;
    type EmuState = CpuV3CoreState;

    const USES_MAIN_CLOCK: bool = true;

    fn create_emu(_input: &Self::Input, _output: &Self::Output) -> Self::EmuState {
        CpuV3CoreState::default()
    }

    fn execute_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        input: &Self::Input,
        output: &Self::Output,
    ) {
        let input = input.sample(circuit);
        let pending = state.pending_data;
        let store = state.async_store;
        let execute_pipelineable = state.execute_pipelineable();
        let device_instruction = state.phase == Phase::Execute && state.instruction >> 12 == 0x7;
        let device_field = field(state.instruction, 8);
        let device_register = field(state.instruction, 0);
        output.drive(
            circuit,
            &CpuV3CoreOutputValue {
                instruction_request_valid: !input.hold
                    && (state.phase == Phase::FetchRequest || execute_pipelineable),
                instruction_address: u64::from(physical_address(state.code_segment, state.pc)),
                instruction_response_ready: !input.hold
                    && (matches!(state.phase, Phase::FetchRequest | Phase::FetchResponse)
                        || execute_pipelineable),
                data_request_valid: !input.hold
                    && ((store.valid && !store.issued) || state.phase == Phase::DataRequest),
                data_write: store.valid || pending.write,
                data_address: u64::from(if store.valid {
                    store.address
                } else {
                    pending.address
                }),
                data_write_data: u64::from(if store.valid {
                    store.write_data
                } else {
                    pending.write_data
                }),
                data_response_ready: !input.hold
                    && ((store.valid && store.issued) || state.phase == Phase::DataResponse),
                device_index: u64::from(device_field & 7),
                device_channel: u64::from(field(state.instruction, 4)),
                device_read_enable: !input.hold && device_instruction && device_field & 8 == 0,
                device_write_enable: !input.hold && device_instruction && device_field & 8 != 0,
                device_write_data: u64::from(state.registers[usize::from(device_register)]),
                halted: state.phase == Phase::Halted && !store.valid,
                halt_signal: u64::from(state.halt_signal),
                fault: state.phase == Phase::Fault,
                fault_code: u64::from(state.fault_code),
                fault_pc: u64::from(state.fault_pc),
                pc: u64::from(state.pc),
                code_segment: u64::from(state.code_segment),
                data_segment: u64::from(state.data_segment),
                retired_words: u64::from(state.retired_words),
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
            *state = CpuV3CoreState::default();
            // The RTL reset walks the scalar register file back to zero over
            // 16 cycles; mirror that instead of clearing it in one step.
            state.phase = Phase::ResetClear;
            return;
        }
        if input.hold {
            return;
        }
        // Land the previous cycle's staged GPR write. The RTL registers the
        // write request at retirement and its synchronous GPR RAM commits one
        // cycle later, during the following fetch cycle.
        if state.gpr_write_enable {
            state.registers[usize::from(state.gpr_write_address)] = state.gpr_write_data;
            state.gpr_write_enable = false;
        }
        let execute_pipelineable = state.execute_pipelineable();
        // Snapshot the pre-clock buffer state to preserve the RTL's
        // nonblocking semantics: a waiter observes completion one cycle after
        // the response handshake, and a newly enqueued store cannot issue on
        // the same edge that created it.
        let async_store_was_valid = state.async_store.valid;
        let async_store_was_issued = state.async_store.issued;
        if async_store_was_valid
            && async_store_was_issued
            && input.data_response_valid
            && input.data_error
        {
            let fault_pc = state.async_store.fault_pc;
            state.async_store = AsyncStore::default();
            state.fault(CPU_V3_FAULT_DATA_MEMORY, fault_pc);
            return;
        }
        if async_store_was_valid && !async_store_was_issued && input.data_request_ready {
            state.async_store.issued = true;
        }
        if async_store_was_valid && async_store_was_issued && input.data_response_valid {
            state.async_store = AsyncStore::default();
        }
        match state.phase {
            Phase::FetchRequest if input.instruction_request_ready => {
                if input.instruction_response_valid {
                    if input.instruction_error {
                        state.fault(CPU_V3_FAULT_INSTRUCTION_MEMORY, state.pc);
                    } else {
                        state.instruction = input.instruction_data as u16;
                        state.instruction_pc = state.pc;
                        state.pc = state.pc.wrapping_add(1);
                        state.phase = Phase::Execute;
                    }
                } else {
                    state.phase = Phase::FetchResponse;
                }
            }
            Phase::FetchResponse if input.instruction_response_valid => {
                if input.instruction_error {
                    state.fault(CPU_V3_FAULT_INSTRUCTION_MEMORY, state.pc);
                } else {
                    state.instruction = input.instruction_data as u16;
                    state.instruction_pc = state.pc;
                    state.pc = state.pc.wrapping_add(1);
                    state.phase = Phase::Execute;
                }
            }
            Phase::Execute => {
                state.execute(input.device_read_data as u16);
                if execute_pipelineable
                    && input.instruction_request_ready
                    && state.phase == Phase::FetchRequest
                {
                    if input.instruction_response_valid {
                        if input.instruction_error {
                            state.fault(CPU_V3_FAULT_INSTRUCTION_MEMORY, state.pc);
                        } else {
                            state.instruction = input.instruction_data as u16;
                            state.instruction_pc = state.pc;
                            state.pc = state.pc.wrapping_add(1);
                            state.phase = Phase::Execute;
                        }
                    } else {
                        state.phase = Phase::FetchResponse;
                    }
                }
            }
            Phase::DataRequest if input.data_request_ready => {
                state.phase = Phase::DataResponse;
            }
            Phase::AsyncStoreWait if !async_store_was_valid => {
                let pending = state.pending_data;
                if pending.write {
                    state.async_store = AsyncStore {
                        valid: true,
                        issued: false,
                        address: pending.address,
                        write_data: pending.write_data,
                        fault_pc: pending.fault_pc,
                    };
                    state.retire(pending.retire_words);
                } else {
                    state.phase = Phase::DataRequest;
                }
            }
            Phase::DataResponse if input.data_response_valid => {
                let pending = state.pending_data;
                if input.data_error {
                    state.fault(CPU_V3_FAULT_DATA_MEMORY, pending.fault_pc);
                } else {
                    if !pending.write {
                        state.write_gpr(pending.destination, input.data_read_data as u16);
                    }
                    state.retire(pending.retire_words);
                }
            }
            Phase::MultiplyWait => state.phase = Phase::MultiplyCommit,
            Phase::MultiplyCommit => {
                state.write_gpr(
                    state.multiply_destination,
                    (state.multiply_product >> state.multiply_shift) as u16,
                );
                state.retire(state.multiply_retire_words);
            }
            Phase::ResetClear => {
                state.write_gpr(state.clear_index, 0);
                if state.clear_index == 15 {
                    state.phase = Phase::FetchRequest;
                } else {
                    state.clear_index += 1;
                }
            }
            _ => {}
        }
    }

    fn verilog_source() -> Option<String> {
        Some(
            include_str!("cpu_v3_core.v")
                .replace(
                    "__DSP_MULTIPLIER__",
                    &DspMulS18::verilog_identity().module_name(),
                )
                .replace(
                    "__GPR_RAM__",
                    &CpuV3GprRam::verilog_identity().module_name(),
                ),
        )
    }

    fn verilog_dependencies() -> Vec<VerilogDependency> {
        vec![
            VerilogDependency::new::<DspMulS18>("u_multiplier"),
            VerilogDependency::new::<CpuV3GprRam>("u_gpr_ram"),
        ]
    }

    fn verilog_testbench() -> Option<String> {
        Some(include_str!("cpu_v3_core_tb.v").to_string())
    }
}

fn physical_address(segment: u16, offset: u16) -> u32 {
    (u32::from(segment) << 16) | u32::from(offset)
}

fn field(instruction: u16, shift: u32) -> u8 {
    ((instruction >> shift) & 15) as u8
}

fn sign_extend(value: u16, bits: u32) -> u16 {
    let shift = u16::BITS - bits;
    (((value << shift) as i16) >> shift) as u16
}

fn immediate4(instruction: u16, prefix: Option<Prefix>, signed: bool) -> u16 {
    prefix.map_or_else(
        || {
            if signed {
                sign_extend(instruction & 15, 4)
            } else {
                instruction & 15
            }
        },
        |value| (value.payload << 4) | (instruction & 15),
    )
}

fn is_prefix_consumer(instruction: u16) -> bool {
    let function = (instruction >> 8) & 15;
    match instruction >> 12 {
        8 | 9 => true,
        2 => function == 12,
        10 => matches!(function, 0..=6 | 8..=10 | 12 | 13),
        11 => matches!(function, 0..=7),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate as cpu_v3;
    use crate::rcc_backend::{self, CompilerOptions};
    use crate::{AluOp, CpuV3Sim, ImmediateOp, RunOutcome, SpecialRegister, TestCondition};
    use digital_design_circuit::{build_circuit, Circuit};
    use digital_design_hardware::VerilogProject;
    use rcc::frontend::compile_program;
    use std::collections::HashMap;

    struct CoreRun {
        halt_signal: u16,
        retired_words: u32,
        code_segment: u16,
        data_segment: u16,
    }

    fn drive(
        circuit: &mut Circuit,
        input: &CpuV3CoreInput,
        instruction_response: Option<u16>,
        data_response: Option<u16>,
        device_read_data: u16,
    ) {
        input.drive(
            circuit,
            &CpuV3CoreInputValue {
                reset: false,
                hold: false,
                instruction_request_ready: true,
                instruction_response_valid: instruction_response.is_some(),
                instruction_data: u64::from(instruction_response.unwrap_or(0)),
                instruction_error: false,
                data_request_ready: true,
                data_response_valid: data_response.is_some(),
                data_read_data: u64::from(data_response.unwrap_or(0)),
                data_error: false,
                device_read_data: u64::from(device_read_data),
            },
        );
    }

    fn run_core(mut memory: HashMap<u32, u16>, maximum_cycles: usize) -> CoreRun {
        let (mut circuit, (input, output)) = build_circuit(|| {
            let input = CpuV3CoreInput::allocate();
            let output = CpuV3Core::emu(&input);
            (input, output)
        });
        let mut instruction_response = None;
        let mut data_response = None;
        drive(&mut circuit, &input, None, None, 0);
        let mut devices = [0u16; 128];

        for _cycle in 0..maximum_cycles {
            circuit.execute_gates();
            let value = output.sample(&circuit);
            if value.fault {
                panic!(
                    "CpuV3 core faulted with code {} at {:#06x}",
                    value.fault_code, value.fault_pc
                );
            }
            if value.halted {
                return CoreRun {
                    halt_signal: value.halt_signal as u16,
                    retired_words: value.retired_words as u32,
                    code_segment: value.code_segment as u16,
                    data_segment: value.data_segment as u16,
                };
            }

            let next_instruction_response = value.instruction_request_valid.then(|| {
                memory
                    .get(&(value.instruction_address as u32))
                    .copied()
                    .unwrap_or(0)
            });
            let next_data_response = if value.data_request_valid {
                let address = value.data_address as u32;
                if value.data_write {
                    memory.insert(address, value.data_write_data as u16);
                    Some(0)
                } else {
                    Some(memory.get(&address).copied().unwrap_or(0))
                }
            } else {
                None
            };
            let device_address =
                ((value.device_index as usize) << 4) | value.device_channel as usize;
            if value.device_write_enable {
                devices[device_address] = value.device_write_data as u16;
            }
            drive(
                &mut circuit,
                &input,
                instruction_response.take(),
                data_response.take(),
                devices[device_address],
            );
            circuit.clock_tick();
            instruction_response = next_instruction_response;
            data_response = next_data_response;
        }
        panic!("CpuV3 core exceeded {maximum_cycles} cycles")
    }

    fn load(memory: &mut HashMap<u32, u16>, base: u32, words: &[u16]) {
        for (offset, word) in words.iter().copied().enumerate() {
            memory.insert(base + offset as u32, word);
        }
    }

    fn compile(source: &str) -> Vec<u16> {
        let options = CompilerOptions::default();
        let frontend = compile_program(source, &options, &mut |_| {
            Err("test source does not use modules".to_string())
        })
        .unwrap();
        rcc_backend::compile(frontend, &options, "main").words
    }

    #[test]
    fn cycle_model_signal_events_are_single_cycle_and_model_only() {
        let mut state = CpuV3CoreState::default();
        state.registers[5] = 0x2a;

        // A nonzero SIGNAL retires as a NOP and records the event.
        state.instruction = cpu_v3::signal(5, 1);
        state.execute(0);
        assert_eq!(state.phase, Phase::FetchRequest);
        assert_eq!(state.retired_words, 1);
        assert_eq!(
            state.signal_event(),
            Some(crate::SignalEvent {
                signal_type: 1,
                value: 0x2a,
            })
        );
        // Reading does not clear the event; take does.
        assert!(state.signal_event().is_some());
        assert!(state.take_signal_event().is_some());
        assert_eq!(state.signal_event(), None);

        // A fresh nonzero SIGNAL re-arms the event; the next executed
        // instruction ends the single-cycle window.
        state.instruction = cpu_v3::signal(5, 15);
        state.execute(0);
        assert_eq!(
            state.signal_event().map(|event| event.signal_type),
            Some(15)
        );
        state.instruction = 0x0322; // ADD r3, r2, r2 (any ordinary instruction)
        state.execute(0);
        assert_eq!(state.signal_event(), None);

        // Type 0 halts and latches the selected register, exactly like the
        // architectural HALT path.
        let mut state = CpuV3CoreState::default();
        state.registers[7] = 0x1234;
        state.instruction = cpu_v3::signal(7, 0);
        state.execute(0);
        assert_eq!(state.phase, Phase::Halted);
        assert_eq!(state.halt_signal, 0x1234);
        assert_eq!(state.retired_words, 1);
        assert_eq!(state.signal_event(), None);
    }

    #[test]
    fn emulator_async_store_overlaps_alu_and_blocks_next_memory_operation() {
        let mut state = CpuV3CoreState::default();
        state.registers[1] = 0x0100;
        state.registers[2] = 10;

        state.instruction = 0x9210; // STORE r2, [r1]
        state.execute(0);
        assert_eq!(state.phase, Phase::FetchRequest);
        assert!(state.async_store.valid);
        assert!(!state.async_store.issued);
        assert_eq!(state.async_store.address, 0x0100);
        assert_eq!(state.async_store.write_data, 10);

        state.instruction = 0x0322; // ADD r3, r2, r2
        state.execute(0);
        assert_eq!(state.phase, Phase::FetchRequest);
        assert!(state.async_store.valid);
        assert!(state.gpr_write_enable);
        assert_eq!(state.gpr_write_data, 20);

        state.instruction = 0x8010; // LOAD r0, [r1]
        state.execute(0);
        assert_eq!(state.phase, Phase::AsyncStoreWait);
        assert!(!state.pending_data.write);
        assert_eq!(state.pending_data.address, 0x0100);
    }

    #[test]
    fn emulator_matches_oracle_for_compiler_control_memory_and_multiply() {
        let program = compile(
            r#"
                static VALUE: u16 = 7;
                fn twice(value: u16) -> u16 { value + value }
                fn main() {
                    let mut total: u16 = 0;
                    let mut i: u16 = 1;
                    while i < 6 {
                        total = total + twice(i);
                        i = i + 1;
                    }
                    halt(total + VALUE + mul_16x4(3, 4));
                }
            "#,
        );
        let mut oracle = CpuV3Sim::default();
        oracle.load_program(0, &program).unwrap();
        let outcome = oracle.run(10_000).unwrap();
        let RunOutcome::Halted { signal, .. } = outcome else {
            panic!("oracle did not halt")
        };

        let mut memory = HashMap::new();
        load(&mut memory, 0, &program);
        // The compiler's static-data initialization runs from code, so the
        // external memory begins with the same zero-filled state as CpuV3Sim.
        let core = run_core(memory, 20_000);
        assert_eq!(core.halt_signal, signal);
        assert_eq!(core.retired_words as u64, oracle.retired_words());
    }

    #[test]
    fn emulator_matches_segmented_fetch_data_and_special_register_semantics() {
        let mut boot = Vec::new();
        boot.extend(cpu_v3::load_immediate16(1, 1));
        boot.extend(cpu_v3::load_immediate16(2, 0x20));
        boot.extend(cpu_v3::load_immediate16(3, 2));
        boot.extend([cpu_v3::write_data_segment(3), cpu_v3::jump_segment(1, 2)]);
        let mut application = vec![
            cpu_v3::read_special(4, SpecialRegister::CodeSegment),
            cpu_v3::read_special(5, SpecialRegister::DataSegment),
        ];
        application.extend(cpu_v3::load_immediate16(6, 0x1234));
        application.extend([cpu_v3::load(0, 6, 0), cpu_v3::halt()]);

        let mut memory = HashMap::new();
        load(&mut memory, 0, &boot);
        load(&mut memory, 0x0001_0020, &application);
        memory.insert(0x0002_1234, 0xbeef);
        let core = run_core(memory, 1_000);
        assert_eq!(core.halt_signal, 0xbeef);
        assert_eq!((core.code_segment, core.data_segment), (1, 2));
    }

    #[test]
    fn emulator_matches_oracle_for_reserved_prefix_and_comparison_edges() {
        let mut program = Vec::new();
        program.extend(cpu_v3::load_immediate16(1, 0x8000));
        program.extend(cpu_v3::load_immediate16(2, 0x7fff));
        program.extend(cpu_v3::load_immediate16(6, 3));
        program.extend(cpu_v3::load_immediate16(7, 5));
        program.extend([
            cpu_v3::move_register(8, 6),
            cpu_v3::multiply(cpu_v3::MultiplyWindow::Low, 8, 7),
            cpu_v3::move_register(3, 1),
            cpu_v3::set_less_than_signed(3, 2),
            cpu_v3::move_register(4, 1),
            cpu_v3::set_less_than_unsigned(4, 2),
            cpu_v3::population_count(5, 1),
            cpu_v3::shift_immediate(cpu_v3::ShiftOp::RightLogical, 1, 15),
            cpu_v3::alu(AluOp::Add, 0, 3, 4),
            cpu_v3::alu(AluOp::Add, 0, 0, 5),
            cpu_v3::alu(AluOp::Add, 0, 0, 8),
            cpu_v3::immediate_signed(ImmediateOp::CompareSigned, 0, 0),
            cpu_v3::branch(TestCondition::NotEqual, 1),
            cpu_v3::immediate_unsigned(ImmediateOp::LoadUnsigned, 0, 0),
            cpu_v3::halt(),
        ]);
        let mut oracle = CpuV3Sim::default();
        oracle.load_program(0, &program).unwrap();
        let RunOutcome::Halted { signal, .. } = oracle.run(1_000).unwrap() else {
            panic!("oracle did not halt")
        };
        let mut memory = HashMap::new();
        load(&mut memory, 0, &program);
        let core = run_core(memory, 2_000);
        assert_eq!(core.halt_signal, signal);
        assert_eq!(core.retired_words as u64, oracle.retired_words());
    }

    #[test]
    fn emulator_matches_oracle_for_dedicated_device_instructions() {
        let mut program = Vec::new();
        program.extend(cpu_v3::load_immediate16(1, 0x1234));
        program.extend([
            cpu_v3::device_send(1, 2, 3),
            cpu_v3::device_receive(0, 2, 3),
            cpu_v3::halt(),
        ]);
        let mut oracle = CpuV3Sim::default();
        oracle.load_program(0, &program).unwrap();
        struct EchoDevice([u16; 16]);
        impl cpu_v3::Device for EchoDevice {
            fn read(&mut self, _memory: &mut [u16], channel: u8) -> u16 {
                self.0[usize::from(channel)]
            }
            fn write(&mut self, _memory: &mut [u16], channel: u8, value: u16) {
                self.0[usize::from(channel)] = value;
            }
            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
        }
        oracle.attach_device(2, Box::new(EchoDevice([0; 16])));
        let RunOutcome::Halted { signal, .. } = oracle.run(1_000).unwrap() else {
            panic!("oracle did not halt")
        };
        assert_eq!(signal, 0x1234);
        let mut memory = HashMap::new();
        load(&mut memory, 0, &program);
        let core = run_core(memory, 1_000);
        assert_eq!(core.halt_signal, signal);
        assert_eq!(core.retired_words as u64, oracle.retired_words());
    }

    #[test]
    #[ignore = "explicit external simulation of the reusable CpuV3 core"]
    fn verify_verilog_with_iverilog() {
        digital_design_hardware::verify_verilog_with_iverilog::<CpuV3Core>().unwrap();
    }

    // ---- emulator vs RTL co-simulation of the CpuV3 core ----
    //
    // The fetch queue and the two-way cache already have cycle-accurate
    // emulator-vs-Icarus co-simulations. The core itself only had separate
    // oracle (emulator) and `cpu_v3_core_tb.v` (RTL) regression suites, which
    // never drove the same stimulus through both. The Stage 12 overlap changed
    // the core's fetch/execute timing and added a GPR forwarding mux, so this
    // test runs the same program through the Rust emulator and the RTL and
    // compares a curated set of deterministic architectural outputs every
    // cycle, including the fetch/execute overlap, the forwarded store value,
    // and the barrier paths.

    #[derive(Clone, Copy, Debug, Default)]
    struct CoreCosimIn {
        reset: bool,
        instruction_request_ready: bool,
        instruction_response_valid: bool,
        instruction_data: u16,
        data_request_ready: bool,
        data_response_valid: bool,
        data_read_data: u16,
        device_read_data: u16,
    }

    impl CoreCosimIn {
        fn into_value(self) -> CpuV3CoreInputValue {
            CpuV3CoreInputValue {
                reset: self.reset,
                hold: false,
                instruction_request_ready: self.instruction_request_ready,
                instruction_response_valid: self.instruction_response_valid,
                instruction_data: u64::from(self.instruction_data),
                instruction_error: false,
                data_request_ready: self.data_request_ready,
                data_response_valid: self.data_response_valid,
                data_read_data: u64::from(self.data_read_data),
                data_error: false,
                device_read_data: u64::from(self.device_read_data),
            }
        }
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    struct CoreCosimOut {
        pc: u16,
        code_segment: u16,
        data_segment: u16,
        retired_words: u32,
        halted: bool,
        halt_signal: u16,
        fault: bool,
        fault_code: u8,
        fault_pc: u16,
        instruction_request_valid: bool,
        instruction_address: u32,
        instruction_response_ready: bool,
        data_request_valid: bool,
        data_write: bool,
        data_address: u32,
        data_write_data: u16,
        data_response_ready: bool,
    }

    impl CoreCosimOut {
        /// `halt_signal` is now a registered value latched at the HALT retire
        /// edge (mirrored by the emulator), so it is a stable architectural
        /// property of every cycle after reset and is compared directly.
        fn equal_core(&self, other: &Self) -> bool {
            self.pc == other.pc
                && self.code_segment == other.code_segment
                && self.data_segment == other.data_segment
                && self.retired_words == other.retired_words
                && self.halted == other.halted
                && self.halt_signal == other.halt_signal
                && self.fault == other.fault
                && self.fault_code == other.fault_code
                && self.fault_pc == other.fault_pc
                && self.instruction_request_valid == other.instruction_request_valid
                && self.instruction_address == other.instruction_address
                && self.instruction_response_ready == other.instruction_response_ready
                && self.data_request_valid == other.data_request_valid
                && self.data_write == other.data_write
                && self.data_address == other.data_address
                && self.data_write_data == other.data_write_data
                && self.data_response_ready == other.data_response_ready
        }
    }

    impl From<&CpuV3CoreOutputValue> for CoreCosimOut {
        fn from(value: &CpuV3CoreOutputValue) -> Self {
            // Device-bus fields are intentionally omitted: they are
            // instruction-decode-derived and are not part of the Stage 12
            // pipeline/forwarding behavior under test.
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
            }
        }
    }

    /// Pipeline-focused program: a wide (SETP) load, a dependent immediate
    /// chain that the forwarding bypass permits to run one instruction per
    /// cycle, control-path ALU ops, a comparison, and a store whose data value
    /// must equal the forwarded `r0`, then halt, followed by ISA 0.8 coverage
    /// for the destructive shift/multiply family, the Boolean comparisons and
    /// conditional moves, non-halting SIGNAL, the special registers, and the
    /// relative/register call-return pair. Keeping both phases here means the
    /// Rust cycle model and the RTL stay compared on every new decoder and
    /// pipeline classification.
    fn core_cosim_program() -> Vec<u16> {
        let mut p = Vec::new();
        p.extend(cpu_v3::load_immediate16(0, 0)); // r0 = 0 (SETP + ADDIU, two physical words)
        p.extend(cpu_v3::load_immediate16(1, 0x4000)); // r1 = 0x4000 (data base)
        for _ in 0..5 {
            // Dependent r0 <- r0 + 1; the forwarding mux lets each read see the
            // previous cycle's pending write so they run at one per cycle.
            p.push(cpu_v3::immediate_unsigned(crate::ImmediateOp::Add, 0, 1));
        }
        p.push(cpu_v3::immediate_unsigned(
            crate::ImmediateOp::CompareUnsigned,
            0,
            5,
        )); // pending test = r0 == 5
        p.push(cpu_v3::branch(crate::TestCondition::Equal, 1)); // taken if r0 == 5
        p.push(cpu_v3::nop()); // not taken
        p.push(cpu_v3::alu(crate::AluOp::Add, 2, 0, 1)); // r2 = r0 + r1 (5 + 0x4000)
        p.push(cpu_v3::move_register(3, 2)); // r3 = r2
        p.push(cpu_v3::not(4, 3)); // r4 = ~r3
        p.push(cpu_v3::negate(5, 4)); // r5 = -r4
        p.push(cpu_v3::store(0, 1, 4)); // mem[r1+4] = r0 (async store, observes forwarded r0 = 5)

        // Destructive shift/multiply family (major 2). `MUL8 rd == rs`
        // exercises the read-before-write window, the immediate shifts cover
        // the 4-bit amount field, and the register form masks `rs & 15`.
        p.extend(cpu_v3::load_immediate16(2, 0x00ff)); // r2 = 0x00ff
        p.extend(cpu_v3::load_immediate16(3, 0x00ff)); // r3 = 0x00ff
        p.push(cpu_v3::multiply(cpu_v3::MultiplyWindow::Low, 2, 3)); // MUL0, rd != rs
        p.push(cpu_v3::multiply(cpu_v3::MultiplyWindow::Shift8, 3, 3)); // MUL8, rd == rs
        p.push(cpu_v3::multiply(cpu_v3::MultiplyWindow::Shift16, 3, 2)); // MUL16
        p.extend(cpu_v3::load_immediate16(4, 0x8001)); // r4 = 0x8001
        p.push(cpu_v3::shift_immediate(
            cpu_v3::ShiftOp::RightLogical,
            4,
            15,
        ));
        p.push(cpu_v3::shift_immediate(cpu_v3::ShiftOp::Left, 4, 15));
        p.push(cpu_v3::shift_immediate(
            cpu_v3::ShiftOp::RightArithmetic,
            4,
            15,
        ));
        p.extend(cpu_v3::load_immediate16(5, 0x0011)); // r5 = 0x11
        p.push(cpu_v3::shift_register(cpu_v3::ShiftOp::RightLogical, 4, 5)); // amount = 0x11 & 15 = 1
        p.push(cpu_v3::multiply_immediate(2, 3)); // MULI, u4
        p.extend(cpu_v3::prefixed(cpu_v3::multiply_immediate(3, 0), 4)); // MULI with PFX12

        // Boolean comparisons and conditional moves consume the pending test
        // whether or not the move writes.
        p.push(cpu_v3::compare_signed(2, 3));
        p.push(cpu_v3::conditional_move(
            crate::TestCondition::GreaterOrEqual,
            6,
            2,
        ));
        p.push(cpu_v3::compare_unsigned(2, 3));
        p.push(cpu_v3::conditional_move(
            crate::TestCondition::LessThan,
            7,
            3,
        ));
        p.push(cpu_v3::set_equal(8, 3)); // SEQ rd, rs
        p.push(cpu_v3::set_less_than_signed(9, 2)); // SLT rd, rs
        p.push(cpu_v3::set_less_than_unsigned(10, 4)); // SLTU rd, rs

        // LDC/ADDC (major A functions 7/B) index the shared constant table
        // and never consume PFX12; the prefix before the final LDC expires
        // unused and retires separately. Both table signs are covered.
        p.push(cpu_v3::load_constant(12, 0)); // r12 = 8
        p.push(cpu_v3::load_constant(11, 15)); // r11 = -512
        p.push(cpu_v3::add_constant(12, 7)); // r12 = 8 + 512 = 520
        p.push(cpu_v3::add_constant(11, 9)); // r11 = -512 + -16 = -528
        p.push(cpu_v3::prefix12(0xabc));
        p.push(cpu_v3::load_constant(12, 0)); // r12 = 8 (prefix expires)

        // ADDI/SUBI read the unprefixed immediate as an unsigned u4; 0 and 15
        // are the range boundaries (15 was -1 under the signed reading).
        p.push(cpu_v3::immediate_unsigned(crate::ImmediateOp::Add, 12, 15)); // r12 = 23
        p.push(cpu_v3::immediate_unsigned(crate::ImmediateOp::Sub, 12, 0)); // r12 = 23
        p.push(cpu_v3::immediate_unsigned(crate::ImmediateOp::Sub, 12, 15)); // r12 = 8
        p.push(cpu_v3::immediate_unsigned(crate::ImmediateOp::Add, 12, 0)); // r12 = 8

        // Non-halting SIGNAL types retire as a NOP in the RTL; both special
        // registers are read and DSEG is rewritten with its current value.
        p.push(cpu_v3::signal(6, 1));
        p.push(cpu_v3::signal(7, 15));
        p.push(cpu_v3::read_special(
            11,
            crate::SpecialRegister::DataSegment,
        ));
        p.push(cpu_v3::write_data_segment(11));
        p.push(cpu_v3::read_special(
            12,
            crate::SpecialRegister::CodeSegment,
        ));

        // JALREL links the fixed r14 and JREG returns through it; the trailing
        // JREL skips the subroutine body once control comes back.
        p.push(cpu_v3::jump_and_link_relative(2)); // r14 = next word, pc = +3
        p.push(cpu_v3::nop()); // return point
        p.push(cpu_v3::jump_relative(1)); // skip the JREG subroutine body
        p.push(cpu_v3::jump_register(cpu_v3::LINK_REGISTER)); // subroutine body: return

        // JALR loads an absolute target, links r14, jumps, and returns through
        // JREG. The subroutine sits after the final halt so fall-through never
        // reaches it.
        let subroutine = (p.len() + 5) as u16;
        p.extend(cpu_v3::load_immediate16(13, subroutine));
        p.push(cpu_v3::jump_and_link_register(13));
        p.push(cpu_v3::alu(crate::AluOp::Add, 0, 2, 3));
        // Fold r12 (= 8 from the LDC/ADDI chain above) into the halt signal so
        // the constant-table and unsigned-immediate results are observed.
        p.push(cpu_v3::alu(crate::AluOp::Add, 0, 0, 12));
        p.push(cpu_v3::halt());
        p.push(cpu_v3::jump_register(cpu_v3::LINK_REGISTER));
        p
    }

    fn run_core_emu_trace(program: &[u16], max_cycles: usize) -> Vec<CoreCosimOut> {
        let mut memory = vec![0u16; 65536];
        for (index, word) in program.iter().copied().enumerate() {
            memory[index] = word;
        }
        let (mut circuit, (input, output)) = build_circuit(|| {
            let input = CpuV3CoreInput::allocate();
            let output = CpuV3Core::emu(&input);
            (input, output)
        });
        let mut prev_instr_req = false;
        let mut prev_instr_word: u16 = 0;
        let mut prev_data_req = false;
        let mut started = false;
        let mut trace = Vec::new();
        for _ in 0..max_cycles {
            let cin = CoreCosimIn {
                reset: false,
                instruction_request_ready: true,
                instruction_response_valid: prev_instr_req,
                instruction_data: prev_instr_word,
                data_request_ready: true,
                data_response_valid: prev_data_req,
                data_read_data: 0,
                device_read_data: 0,
            };
            input.drive(&mut circuit, &cin.into_value());
            circuit.execute_gates();
            let value = output.sample(&circuit);
            if started || value.instruction_request_valid {
                started = true;
                trace.push(CoreCosimOut::from(&value));
                if value.halted || value.fault {
                    break;
                }
            }
            prev_instr_req = value.instruction_request_valid;
            if value.instruction_request_valid {
                prev_instr_word = memory[(value.instruction_address as usize) & 0xffff];
            }
            prev_data_req = value.data_request_valid;
            if value.data_request_valid && value.data_write {
                memory[(value.data_address as usize) & 0xffff] = value.data_write_data as u16;
            }
            circuit.clock_tick();
        }
        trace
    }

    /// Generates a testbench that runs the RTL core with the same 1-cycle
    /// instruction/data memory semantics used by `run_core_emu_trace`, resets,
    /// then records the same curated output set each cycle from the first
    /// emitted instruction request until halt/fault.
    fn build_core_cosim_tb(program: &[u16], module_name: &str, max_cycles: usize) -> String {
        let mut t = format!(
            "module tb;\n\
             reg clk = 0;\n\
             reg reset = 1;\n\
             reg hold = 0;\n\
             reg instruction_request_ready = 1;\n\
             reg instruction_response_valid = 0;\n\
             reg [15:0] instruction_data = 0;\n\
             reg instruction_error = 0;\n\
             reg data_request_ready = 1;\n\
             reg data_response_valid = 0;\n\
             reg [15:0] data_read_data = 0;\n\
             reg data_error = 0;\n\
             wire [15:0] device_read_data;\n\
             wire instruction_request_valid;\n\
             wire [31:0] instruction_address;\n\
             wire instruction_response_ready;\n\
             wire data_request_valid;\n\
             wire data_write;\n\
             wire [31:0] data_address;\n\
             wire [15:0] data_write_data;\n\
             wire data_response_ready;\n\
             wire [2:0] device_index;\n\
             wire [3:0] device_channel;\n\
             wire device_read_enable;\n\
             wire device_write_enable;\n\
             wire [15:0] device_write_data;\n\
             wire halted;\n\
             wire [15:0] halt_signal;\n\
             wire fault;\n\
             wire [7:0] fault_code;\n\
             wire [15:0] fault_pc;\n\
             wire [15:0] pc;\n\
             wire [15:0] code_segment;\n\
             wire [15:0] data_segment;\n\
             wire [31:0] retired_words;\n\n\
             {module_name} dut(.*);\n\
             always #5 clk = ~clk;\n\n\
             reg [15:0] memory [0:65535];\n\
             reg [15:0] devices [0:127];\n\
             assign device_read_data = devices[{{device_index, device_channel}}];\n\
             integer index;\n\
             integer cycles;\n\
             reg started;\n\
             reg end_flag;\n\n\
             always @(posedge clk) begin\n\
                 instruction_response_valid <= instruction_request_valid;\n\
                 if (instruction_request_valid)\n\
                     instruction_data <= memory[instruction_address[15:0]];\n\
                 data_response_valid <= data_request_valid;\n\
                 if (data_request_valid && data_write)\n\
                     memory[data_address[15:0]] <= data_write_data;\n\
                 else if (data_request_valid)\n\
                     data_read_data <= memory[data_address[15:0]];\n\
             end\n\n\
             initial begin\n",
        );
        for (index, word) in program.iter().copied().enumerate() {
            t.push_str(&format!("    memory[{index}] = 16'h{word:04x};\n"));
        }
        t.push_str(&format!(
            "    for (index = 0; index < 128; index = index + 1) devices[index] = 16'h0000;\n\
             repeat (3) @(posedge clk);\n\
             #1 reset = 0;\n\
             cycles = 0;\n\
             started = 0;\n\
             end_flag = 0;\n\
             while (cycles < {max_cycles} && !end_flag) begin\n\
                 #1;\n\
                 if (started || instruction_request_valid)\n\
                     started = 1;\n\
                 if (started) begin\n\
                     $display(\"CORE %0d %0d %0d %0d %0d %0d %0d %0d %0d %0d %0d %0d %0d %0d %0d %0d %0d %0d\", cycles, pc, code_segment, data_segment, retired_words, halted, halt_signal, fault, fault_code, fault_pc, instruction_request_valid, instruction_address, instruction_response_ready, data_request_valid, data_write, data_address, data_write_data, data_response_ready);\n\
                     if (halted || fault) end_flag = 1;\n\
                 end\n\
                 @(posedge clk);\n\
                 cycles = cycles + 1;\n\
             end\n\
             $display(\"TRACE_END\");\n\
             $finish;\n\
             end\n\
              initial begin\n\
                  repeat ({timeout_cycles}) @(posedge clk);\n\
                  $display(\"TIMEOUT\");\n\
                  $finish(1);\n\
              end\n\
              endmodule\n",
            timeout_cycles = max_cycles * 5 + 500
        ));
        t
    }

    fn collect_verilog_files(directory: &std::path::Path, into: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                collect_verilog_files(&path, into);
            } else if path.extension().and_then(|value| value.to_str()) == Some("v")
                && path.file_name().and_then(|value| value.to_str()) != Some("tb.v")
            {
                into.push(path);
            }
        }
    }

    fn run_core_rtl_trace(tb: &str) -> Vec<CoreCosimOut> {
        let directory = std::env::temp_dir().join(format!("core-cosim-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        // Write the core with every dependency (DSP multiplier, GPR RAM)
        // via the same flattening used by `verify_verilog_with_iverilog`, then
        // add our replay testbench.
        let project = VerilogProject::generate::<CpuV3Core>().unwrap();
        project.write_to(&directory).unwrap();
        std::fs::write(directory.join("tb.v"), tb).unwrap();
        let iverilog = std::env::var_os("IVERILOG_EXE").unwrap_or_else(|| "iverilog".into());
        let vvp = std::env::var_os("VVP_EXE").unwrap_or_else(|| "vvp".into());
        let output_path = directory.join("sim.vvp");
        // The generated project writes dependency files under nested paths; walk
        // the tree to gather every module source.
        let mut module_paths = Vec::new();
        collect_verilog_files(&directory, &mut module_paths);
        module_paths.push(directory.join("tb.v"));
        let mut compile = std::process::Command::new(&iverilog);
        compile
            .current_dir(&directory)
            .args(["-g2005", "-s", "tb", "-o"])
            .arg(&output_path);
        for path in module_paths {
            compile.arg(&path);
        }
        let compile_output = compile.output().unwrap();
        assert!(
            compile_output.status.success(),
            "iverilog compile failed:\n{}",
            String::from_utf8_lossy(&compile_output.stderr)
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
        let mut trace = Vec::new();
        for line in stdout.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("CORE ") {
                let fields: Vec<&str> = rest.split_whitespace().collect();
                assert_eq!(fields.len(), 18, "unexpected CORE line: {line}");
                let num = |i: usize| fields[i].parse().unwrap_or(0);
                trace.push(CoreCosimOut {
                    pc: num(1) as u16,
                    code_segment: num(2) as u16,
                    data_segment: num(3) as u16,
                    retired_words: num(4) as u32,
                    halted: num(5) == 1,
                    halt_signal: num(6) as u16,
                    fault: num(7) == 1,
                    fault_code: num(8) as u8,
                    fault_pc: num(9) as u16,
                    instruction_request_valid: num(10) == 1,
                    instruction_address: num(11) as u32,
                    instruction_response_ready: num(12) == 1,
                    data_request_valid: num(13) == 1,
                    data_write: num(14) == 1,
                    data_address: num(15) as u32,
                    data_write_data: num(16) as u16,
                    data_response_ready: num(17) == 1,
                });
            } else if line == "TRACE_END" {
                break;
            }
        }
        std::fs::remove_dir_all(&directory).ok();
        trace
    }

    #[test]
    fn core_cosim_program_halts_in_the_simulators() {
        // The ignored emu/RTL co-simulation only compares well-defined runs, so
        // keep its program valid, in-range, and terminating. This checks the
        // program against the architectural oracle and the Rust cycle model
        // (the emu half of the co-sim) without needing Icarus, and pins the
        // cycle count below the emu trace cap used by the co-sim.
        let program = core_cosim_program();
        let mut machine = CpuV3Sim::with_physical_memory_words(1 << 16);
        machine.load_program(0, &program).unwrap();
        assert!(matches!(machine.run(10_000), Ok(RunOutcome::Halted { .. })));

        let emu = run_core_emu_trace(&program, 10_000);
        let last = emu.last().copied().expect("emu trace empty");
        assert!(!last.fault, "co-sim program faulted in the cycle model");
        assert!(
            last.halted,
            "co-sim program did not halt in the cycle model"
        );
        assert!(
            emu.len() < 2000,
            "co-sim program needs {} cycles, above the co-sim emu trace cap",
            emu.len()
        );
    }

    #[test]
    #[ignore = "explicit emulator-vs-Icarus co-simulation of the CpuV3 core pipeline"]
    fn core_emu_matches_rtl_pipeline_overlap() {
        let program = core_cosim_program();
        let module_name = CpuV3Core::verilog_identity().module_name();
        let emu = run_core_emu_trace(&program, 2000);
        // A dependent-immediate chain plus a device handshake need a bounded
        // but generous window.
        let max_cycles = emu.len() + 400;
        let tb = build_core_cosim_tb(&program, &module_name, max_cycles);
        let rtl = run_core_rtl_trace(&tb);
        assert!(
            emu.len() == rtl.len(),
            "emu/RTL trace length mismatch: emu={} rtl={}\nemu={:?}\nrtl={:?}",
            emu.len(),
            rtl.len(),
            emu.iter().map(|v| v.pc).collect::<Vec<_>>(),
            rtl.iter().map(|v| v.pc).collect::<Vec<_>>()
        );
        for (index, (expected, actual)) in emu.iter().zip(&rtl).enumerate() {
            assert!(
                actual.equal_core(expected),
                "emu/RTL core mismatch at cycle {index}\nemu={expected:?}\nrtl={actual:?}"
            );
        }
        let last_emu = emu.last().copied().expect("emu trace empty");
        assert!(last_emu.halted, "core co-sim must end in halt");
    }

    #[test]
    #[ignore = "explicit external simulation of the scalar register file"]
    fn verify_gpr_ram_with_iverilog() {
        digital_design_hardware::verify_verilog_with_iverilog::<CpuV3GprRam>().unwrap();
    }

    #[test]
    #[ignore = "explicit external simulation of the FPU v2 register file"]
    fn verify_fpu_v2_register_ram_with_iverilog() {
        digital_design_hardware::verify_verilog_with_iverilog::<CpuV3FpuV2RegisterRam>().unwrap();
    }

    #[test]
    #[ignore = "explicit external simulation of the FPU v2 instruction front-end"]
    fn verify_fpu_v2_frontend_with_iverilog() {
        digital_design_hardware::verify_verilog_with_iverilog::<CpuV3FpuV2Frontend>().unwrap();
    }

    #[test]
    #[ignore = "explicit external simulation of the FPU v2 scalar ALU"]
    fn verify_fpu_v2_scalar_alu_with_iverilog() {
        digital_design_hardware::verify_verilog_with_iverilog::<CpuV3FpuV2ScalarAlu>().unwrap();
    }

    #[test]
    #[ignore = "explicit external simulation of the FPU v2 scalar path"]
    fn verify_fpu_v2_scalar_path_with_iverilog() {
        digital_design_hardware::verify_verilog_with_iverilog::<CpuV3FpuV2ScalarPath>().unwrap();
    }

    #[test]
    #[ignore = "explicit external simulation of the FPU v2 unit top"]
    fn verify_fpu_v2_unit_with_iverilog() {
        digital_design_hardware::verify_verilog_with_iverilog::<CpuV3FpuV2>().unwrap();
    }
}
