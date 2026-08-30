//! Reusable CpuV3 revision 0.7 processor core with physical-memory and device ports.

mod cache;
mod fetch;
pub use cache::*;
pub use fetch::*;

use digital_design_circuit::{CircuitWires, Wire, Wires};
use digital_design_hardware::{
    resources::components::SsramBits, HardwareIdentity, Module, ModuleIo, TargetResourceRequest,
    VerilogDependency, VerilogIdentity,
};
use digital_design_hardware_gowin::{Bsram1Rw1024, BsramImage, DspMulS18};
use std::cmp::Ordering;

use crate::{
    acc_saturate, fix16_abs, fix16_add, fix16_ceil, fix16_floor, fix16_from_acc, fix16_neg,
    fix16_reciprocal, fix16_reciprocal_sqrt, fix16_round, fix16_saturate, fix16_saturate01,
    fix16_sign, fix16_sin_cos, fix16_sub, round_shift_ties_even, Fix16Raw, FpuVector,
};

pub const CPU_V3_FAULT_INVALID_INSTRUCTION: u8 = 1;
pub const CPU_V3_FAULT_FPU_DOMAIN: u8 = 2;
pub const CPU_V3_FAULT_INSTRUCTION_MEMORY: u8 = 3;
pub const CPU_V3_FAULT_DATA_MEMORY: u8 = 4;

#[derive(Clone, ModuleIo)]
pub struct CpuV3CoreInput {
    pub reset: Wire,
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

struct FpuRomImage;

impl BsramImage<16> for FpuRomImage {
    const WORDS: [u64; 1024] = crate::FPU_ROM_WORDS;
}

type FpuRom = Bsram1Rw1024<16, FpuRomImage>;

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

/// SSRAM physical bits for the FPU register file: 16 vectors x 64 bits with
/// two asynchronous read ports, which Gowin builds from two RAM16X4 copies
/// (32 cells).
const CPU_V3_FPU_REGISTER_RAM16S: usize = 2 * 4 * 4;
const CPU_V3_FPU_REGISTER_PHYSICAL_BITS: usize = CPU_V3_FPU_REGISTER_RAM16S * 64;

#[derive(Clone, ModuleIo)]
pub struct CpuV3FpuRegisterRamInput {
    /// Per-lane write enables; lane k covers bits [16k, 16k+15].
    pub write_enable: Wires<4>,
    pub write_address: Wires<4>,
    pub write_data: Wires<64>,
    pub read_a_address: Wires<4>,
    pub read_b_address: Wires<4>,
}

#[derive(Clone, ModuleIo)]
pub struct CpuV3FpuRegisterRamOutput {
    pub read_a_data: Wires<64>,
    pub read_b_data: Wires<64>,
}

/// Synchronous-write, dual-asynchronous-read SSRAM holding the sixteen
/// four-lane F registers as whole 64-bit vectors. Full-vector reads and
/// per-lane writes let data-movement instructions move a vec4 per cycle.
pub struct CpuV3FpuRegisterRam;

impl HardwareIdentity for CpuV3FpuRegisterRam {
    const TARGET_RESOURCE_LEAF: bool = true;

    fn verilog_identity() -> VerilogIdentity {
        VerilogIdentity::new("CpuV3FpuRegisterRam").namespace(["components", "cpu", "cpu_v3"])
    }
}

impl Module for CpuV3FpuRegisterRam {
    type Input = CpuV3FpuRegisterRamInput;
    type Output = CpuV3FpuRegisterRamOutput;
    type EmuState = [u16; 64];

    const USES_MAIN_CLOCK: bool = true;

    fn target_resources() -> Vec<TargetResourceRequest> {
        vec![TargetResourceRequest::new(SsramBits::new(
            CPU_V3_FPU_REGISTER_PHYSICAL_BITS as u64,
        ))]
    }

    fn create_emu(_input: &Self::Input, _output: &Self::Output) -> Self::EmuState {
        [0; 64]
    }

    fn execute_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        input: &Self::Input,
        output: &Self::Output,
    ) {
        let input = input.sample(circuit);
        let vector = |address: u64| {
            let base = address as usize * 4;
            u64::from(state[base])
                | u64::from(state[base + 1]) << 16
                | u64::from(state[base + 2]) << 32
                | u64::from(state[base + 3]) << 48
        };
        output.drive(
            circuit,
            &CpuV3FpuRegisterRamOutputValue {
                read_a_data: vector(input.read_a_address),
                read_b_data: vector(input.read_b_address),
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
        let base = input.write_address as usize * 4;
        for lane in 0..4 {
            if input.write_enable >> lane & 1 == 1 {
                state[base + lane as usize] = (input.write_data >> (lane * 16)) as u16;
            }
        }
    }

    fn verilog_source() -> Option<String> {
        Some(include_str!("cpu_v3_fpu_register_ram.v").to_string())
    }

    fn verilog_testbench() -> Option<String> {
        Some(include_str!("cpu_v3_fpu_register_ram_tb.v").to_string())
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
    MultiplyWait,
    MultiplyCommit,
    FpuExecute,
    FpuUnaryDispatch,
    FpuWriteLanes,
    FpuGatherRead,
    FpuGatherWrite,
    FpuScatter,
    FpuTranspose,
    FpuMultiplyWait,
    FpuMultiplySettle,
    FpuMultiplyCommit,
    FpuMultiplyPipeline,
    FpuRomNormalize,
    FpuRomAddress,
    FpuRomLookup,
    FpuRomWait,
    FpuRomCommit,
    FpuRomWrite,
    FpuCommit,
    ResetClear,
    Halted,
    Fault,
}

#[derive(Clone, Copy)]
struct Prefix {
    address: u16,
    high: u16,
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

pub struct CpuV3CoreState {
    registers: [u16; 16],
    gpr_write_enable: bool,
    gpr_write_address: u8,
    gpr_write_data: u16,
    fpu_registers: [FpuVector; 16],
    pc: u16,
    code_segment: u16,
    data_segment: u16,
    prefix: Option<Prefix>,
    /// Transient result of the last CMP-class instruction; mirrors the
    /// architectural `Machine::pending_test` (consumed by conditional
    /// branches, expired by any other retired non-prefix instruction).
    pending_test: Option<Ordering>,
    phase: Phase,
    instruction: u16,
    instruction_pc: u16,
    pending_data: PendingData,
    multiply_destination: u8,
    multiply_result: u16,
    multiply_retire_words: u8,
    fpu_step: u8,
    fpu_accumulator: i64,
    fpu_memory_active: bool,
    fpu_memory_lane: u8,
    fpu_memory_value: FpuVector,
    fpu_scalar: u16,
    fpu_rom_step: u8,
    fpu_rom_first: Fix16Raw,
    fpu_rom_second: Fix16Raw,
    fpu_transpose_rows: [FpuVector; 4],
    fpu_operand_a: Fix16Raw,
    fpu_operand_b: Fix16Raw,
    fpu_result: Fix16Raw,
    fpu_mul_valid: u8,
    fpu_mul_tags: [u8; 2],
    fpu_mul_products: [i64; 2],
    fpu_clear_index: u8,
    fpu_retire_words: u8,
    fpu_fault_pc: u16,
    retired_words: u32,
    fault_code: u8,
    fault_pc: u16,
}

impl Default for CpuV3CoreState {
    fn default() -> Self {
        Self {
            registers: [0; 16],
            gpr_write_enable: false,
            gpr_write_address: 0,
            gpr_write_data: 0,
            fpu_registers: [[0; 4]; 16],
            pc: 0,
            code_segment: 0,
            data_segment: 0,
            prefix: None,
            pending_test: None,
            phase: Phase::FetchRequest,
            instruction: 0,
            instruction_pc: 0,
            pending_data: PendingData::default(),
            multiply_destination: 0,
            multiply_result: 0,
            multiply_retire_words: 0,
            fpu_step: 0,
            fpu_accumulator: 0,
            fpu_memory_active: false,
            fpu_memory_lane: 0,
            fpu_memory_value: [0; 4],
            fpu_scalar: 0,
            fpu_rom_step: 0,
            fpu_rom_first: 0,
            fpu_rom_second: 0,
            fpu_transpose_rows: [[0; 4]; 4],
            fpu_operand_a: 0,
            fpu_operand_b: 0,
            fpu_result: 0,
            fpu_mul_valid: 0,
            fpu_mul_tags: [0; 2],
            fpu_mul_products: [0; 2],
            fpu_clear_index: 0,
            fpu_retire_words: 0,
            fpu_fault_pc: 0,
            retired_words: 0,
            fault_code: 0,
            fault_pc: 0,
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
        if opcode == 0xf {
            if self.prefix.is_some() {
                self.retired_words = self.retired_words.wrapping_add(1);
            }
            self.prefix = Some(Prefix {
                address: self.instruction_pc,
                high: instruction & 0x0fff,
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
            0..=7 if opcode != 2 => {
                let left = self.registers[usize::from(lhs)];
                let right = self.registers[usize::from(rhs)];
                self.write_gpr(dst, match opcode {
                    0 => left.wrapping_add(right),
                    1 => left.wrapping_sub(right),
                    3 => left & right,
                    4 => left | right,
                    5 => left ^ right,
                    6 => left.wrapping_shl(u32::from(right & 15)),
                    7 => ((left as i16) >> u32::from(right & 15)) as u16,
                    _ => unreachable!(),
                });
                self.retire(retire_words);
            }
            2 => self.begin_multiply(
                dst,
                self.registers[usize::from(lhs)],
                self.registers[usize::from(rhs)],
                retire_words,
            ),
            8 | 9 => {
                let offset = immediate4(instruction, prefix, true);
                let logical = self.registers[usize::from(lhs)].wrapping_add(offset);
                self.pending_data = PendingData {
                    write: opcode == 9,
                    address: physical_address(self.data_segment, logical),
                    write_data: self.registers[usize::from(dst)],
                    destination: dst,
                    retire_words,
                    fault_pc,
                };
                self.phase = Phase::DataRequest;
            }
            10 => self.execute_immediate(instruction, prefix, retire_words, fault_pc),
            11 => {
                let condition = dst;
                let offset = prefix.map_or_else(
                    || sign_extend(instruction & 0xff, 8),
                    |value| ((value.high & 0xff) << 8) | (instruction & 0xff),
                );
                match condition {
                    // Conditional branches consume the pending test result.
                    0..=5 => {
                        let Some(test) = pending else {
                            self.fault(CPU_V3_FAULT_INVALID_INSTRUCTION, fault_pc);
                            return;
                        };
                        let taken = match condition {
                            0 => test == Ordering::Equal,
                            1 => test != Ordering::Equal,
                            2 => test == Ordering::Less,
                            3 => test != Ordering::Less,
                            4 => test == Ordering::Greater,
                            5 => test != Ordering::Greater,
                            _ => unreachable!(),
                        };
                        if taken {
                            self.pc = self.pc.wrapping_add(offset);
                        }
                    }
                    // JREL: unconditional relative jump, no link.
                    8 => self.pc = self.pc.wrapping_add(offset),
                    // JALREL: link the fall-through address into r14.
                    9 => {
                        let next = self.pc;
                        self.pc = next.wrapping_add(offset);
                        self.write_gpr(14, next);
                    }
                    _ => {
                        self.fault(CPU_V3_FAULT_INVALID_INSTRUCTION, fault_pc);
                        return;
                    }
                }
                self.retire(retire_words);
            }
            12 => {
                if dst & 8 == 0 {
                    self.write_gpr(rhs, device_read_data);
                }
                self.retire(retire_words);
            }
            13 => {
                self.fpu_retire_words = retire_words;
                self.fpu_fault_pc = fault_pc;
                self.fpu_step = 0;
                self.phase = Phase::FpuExecute;
            }
            14 => self.execute_control(instruction, retire_words, fault_pc),
            _ => self.fault(CPU_V3_FAULT_INVALID_INSTRUCTION, fault_pc),
        }
    }

    fn begin_multiply(&mut self, destination: u8, left: u16, right: u16, retire_words: u8) {
        self.multiply_destination = destination;
        self.multiply_result = left.wrapping_mul(right);
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
        if function == 8 {
            self.begin_multiply(dst, old, signed, retire_words);
            return;
        }
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
            0 => old.wrapping_add(signed),
            1 => old.wrapping_sub(signed),
            2 => old & unsigned,
            3 => old | unsigned,
            4 => old ^ unsigned,
            5 => old.wrapping_shl(u32::from(instruction & 15)),
            6 => old.wrapping_shr(u32::from(instruction & 15)),
            7 => ((old as i16) >> u32::from(instruction & 15)) as u16,
            9 => u16::from(old == signed),
            10 => u16::from((old as i16) < (signed as i16)),
            11 => u16::from(old < unsigned),
            14 if prefix.is_some() => unsigned,
            14 => sign_extend(instruction & 15, 4),
            15 => unsigned,
            _ => {
                self.fault(CPU_V3_FAULT_INVALID_INSTRUCTION, fault_pc);
                return;
            }
        };
        self.write_gpr(dst, result);
        self.retire(retire_words);
    }

    fn execute_control(&mut self, instruction: u16, retire_words: u8, fault_pc: u16) {
        let function = field(instruction, 8);
        let dst = field(instruction, 4);
        let src = field(instruction, 0);
        match function {
            0 => {
                self.write_gpr(
                    dst,
                    self.registers[usize::from(src)].count_ones() as u16,
                )
            }
            1 => self.write_gpr(dst, self.registers[usize::from(src)]),
            2 => self.write_gpr(dst, !self.registers[usize::from(src)]),
            3 => self.write_gpr(dst, self.registers[usize::from(src)].wrapping_neg()),
            4 if dst == 0 => self.pc = self.registers[usize::from(src)],
            // JALR: the link field is architecturally fixed to r14.
            5 if dst == 14 => {
                let target = self.registers[usize::from(src)];
                self.write_gpr(dst, self.pc);
                self.pc = target;
            }
            6 => {
                self.write_gpr(
                    dst,
                    sign_extend(self.registers[usize::from(src)] & 0xff, 8),
                )
            }
            7 => {
                self.write_gpr(
                    dst,
                    self.registers[usize::from(src)].leading_zeros() as u16,
                )
            }
            8 if dst == 0 && src == 0 => {
                self.retired_words = self.retired_words.wrapping_add(u32::from(retire_words));
                self.phase = Phase::Halted;
                return;
            }
            9 => {
                self.write_gpr(
                    dst,
                    u16::from(
                        (self.registers[usize::from(dst)] as i16)
                            < (self.registers[usize::from(src)] as i16),
                    ),
                )
            }
            10 => {
                self.write_gpr(
                    dst,
                    u16::from(self.registers[usize::from(dst)] < self.registers[usize::from(src)]),
                )
            }
            11 => {
                self.pending_test = Some(
                    (self.registers[usize::from(dst)] as i16)
                        .cmp(&(self.registers[usize::from(src)] as i16)),
                )
            }
            12 => {
                self.pending_test =
                    Some(self.registers[usize::from(dst)].cmp(&self.registers[usize::from(src)]))
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

    fn execute_fpu_base(&mut self) {
        let function = field(self.instruction, 8);
        let a = usize::from(field(self.instruction, 4));
        let b = usize::from(field(self.instruction, 0));
        match function {
            0 => {
                // FLOAD: one wide write, lane zero plus cleared lanes.
                self.fpu_registers[a] = [self.registers[b] as i16, 0, 0, 0];
                self.phase = Phase::FpuCommit;
            }
            1 => {
                self.write_gpr(a as u8, self.fpu_registers[b][0] as u16);
                self.phase = Phase::FpuCommit;
            }
            2 | 3 => {
                let offset = self.registers[b];
                if offset & 3 != 0 {
                    self.fault(CPU_V3_FAULT_DATA_MEMORY, self.fpu_fault_pc);
                    return;
                }
                self.fpu_memory_active = true;
                self.fpu_memory_lane = 0;
                self.pending_data = PendingData {
                    write: function == 3,
                    address: physical_address(self.data_segment, offset),
                    write_data: self.fpu_registers[a][0] as u16,
                    destination: a as u8,
                    retire_words: self.fpu_retire_words,
                    fault_pc: self.fpu_fault_pc,
                };
                self.phase = Phase::DataRequest;
            }
            4 => {
                // FMOV: one wide vector copy.
                self.fpu_registers[a] = self.fpu_registers[b];
                self.phase = Phase::FpuCommit;
            }
            5 if b <= 12 => {
                // Pack4: the dispatch port already reads Fb; the remaining
                // snapshot reads run two vectors per cycle.
                self.fpu_memory_value[0] = self.fpu_registers[b][0];
                self.fpu_step = 0;
                self.phase = Phase::FpuGatherRead;
            }
            6 if a <= 12 => {
                // Unpack4 snapshots the source vector so a destination range
                // overlapping Fb stays snapshot-clean.
                self.fpu_memory_value = self.fpu_registers[b];
                self.fpu_step = 0;
                self.phase = Phase::FpuScatter;
            }
            7 if a <= 12 && b == 0 => {
                // Transpose snapshots all four rows before any write.
                self.fpu_transpose_rows[0] = self.fpu_registers[a];
                self.fpu_step = 0;
                self.phase = Phase::FpuTranspose;
            }
            8 | 9 => {
                self.fpu_operand_a = self.fpu_registers[a][0];
                self.fpu_operand_b = self.fpu_registers[b][0];
                self.fpu_step = 0;
                self.phase = Phase::FpuWriteLanes;
            }
            10 | 11 | 15 => {
                // Latch the broadcast scalar: earlier lane commits may
                // overwrite Fb.x when Fa and Fb alias.
                self.fpu_scalar = self.fpu_registers[b][0] as u16;
                self.begin_fpu_multiply_pipeline();
            }
            12 if b <= 3 => {
                self.fpu_registers[a][b] = fix16_from_acc(self.fpu_accumulator);
                self.fpu_accumulator = 0;
                self.phase = Phase::FpuCommit;
            }
            13 => {
                self.pending_test = Some(self.fpu_registers[a][0].cmp(&self.fpu_registers[b][0]));
                self.phase = Phase::FpuCommit;
            }
            14 => match b {
                0 | 1 => {
                    self.fpu_operand_a = self.fpu_registers[a][0];
                    self.fpu_rom_step = 0;
                    self.phase = Phase::FpuUnaryDispatch;
                }
                2 => {
                    self.fpu_rom_step = 0;
                    self.fpu_step = 0;
                    self.phase = Phase::FpuMultiplyWait;
                }
                3..=10 => {
                    self.fpu_operand_a = self.fpu_registers[a][0];
                    self.fpu_operand_b = self.fpu_registers[b][0];
                    self.fpu_step = 0;
                    self.phase = Phase::FpuWriteLanes;
                }
                _ => {
                    self.fault(CPU_V3_FAULT_INVALID_INSTRUCTION, self.fpu_fault_pc);
                }
            },
            _ => self.fault(CPU_V3_FAULT_INVALID_INSTRUCTION, self.fpu_fault_pc),
        }
    }

    fn fpu_product(&self, lane: usize) -> i64 {
        let function = field(self.instruction, 8);
        let a = usize::from(field(self.instruction, 4));
        let b = usize::from(field(self.instruction, 0));
        let right = if function == 15 {
            self.fpu_scalar as i16
        } else {
            self.fpu_registers[b][lane]
        };
        i64::from(self.fpu_registers[a][lane]) * i64::from(right)
    }

    fn begin_fpu_multiply_pipeline(&mut self) {
        self.fpu_step = 1;
        self.fpu_mul_valid = 0b01;
        self.fpu_mul_tags[0] = 0;
        self.fpu_mul_products[0] = self.fpu_product(0);
        self.phase = Phase::FpuMultiplyPipeline;
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
        _input: &Self::Input,
        output: &Self::Output,
    ) {
        let pending = state.pending_data;
        let device_instruction = state.phase == Phase::Execute && state.instruction >> 12 == 0xc;
        let device_field = field(state.instruction, 8);
        let device_register = field(state.instruction, 0);
        output.drive(
            circuit,
            &CpuV3CoreOutputValue {
                instruction_request_valid: state.phase == Phase::FetchRequest,
                instruction_address: u64::from(physical_address(state.code_segment, state.pc)),
                instruction_response_ready: matches!(
                    state.phase,
                    Phase::FetchRequest | Phase::FetchResponse
                ),
                data_request_valid: state.phase == Phase::DataRequest,
                data_write: pending.write,
                data_address: u64::from(pending.address),
                data_write_data: u64::from(pending.write_data),
                data_response_ready: state.phase == Phase::DataResponse,
                device_index: u64::from(device_field & 7),
                device_channel: u64::from(field(state.instruction, 4)),
                device_read_enable: device_instruction && device_field & 8 == 0,
                device_write_enable: device_instruction && device_field & 8 != 0,
                device_write_data: u64::from(state.registers[usize::from(device_register)]),
                halted: state.phase == Phase::Halted,
                halt_signal: u64::from(state.registers[0]),
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
            // The RTL reset walks the FPU register file back to zero over
            // 64 cycles; mirror that instead of clearing it in one step.
            state.phase = Phase::ResetClear;
            return;
        }
        // Land the previous cycle's staged GPR write. The RTL registers the
        // write request at retirement and its synchronous GPR RAM commits one
        // cycle later, during the following fetch cycle.
        if state.gpr_write_enable {
            state.registers[usize::from(state.gpr_write_address)] = state.gpr_write_data;
            state.gpr_write_enable = false;
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
            Phase::Execute => state.execute(input.device_read_data as u16),
            Phase::DataRequest if input.data_request_ready => {
                state.phase = Phase::DataResponse;
            }
            Phase::DataResponse if input.data_response_valid => {
                let pending = state.pending_data;
                if input.data_error {
                    state.fpu_memory_active = false;
                    state.fault(CPU_V3_FAULT_DATA_MEMORY, pending.fault_pc);
                } else if state.fpu_memory_active {
                    let lane = usize::from(state.fpu_memory_lane);
                    if !pending.write {
                        state.fpu_memory_value[lane] = input.data_read_data as i16;
                    }
                    if state.fpu_memory_lane == 3 {
                        state.fpu_memory_active = false;
                        if !pending.write {
                            // Imported beats land in the register file as one
                            // wide vector after the fourth transfer confirmed.
                            state.fpu_registers[usize::from(pending.destination)] = [
                                state.fpu_memory_value[0],
                                state.fpu_memory_value[1],
                                state.fpu_memory_value[2],
                                input.data_read_data as i16,
                            ];
                            state.phase = Phase::FpuCommit;
                        } else {
                            state.retire(pending.retire_words);
                        }
                    } else {
                        state.fpu_memory_lane += 1;
                        state.pending_data.address = pending.address.wrapping_add(1);
                        state.pending_data.write_data = state.fpu_registers
                            [usize::from(field(state.instruction, 4))]
                            [usize::from(state.fpu_memory_lane)]
                            as u16;
                        state.phase = Phase::DataRequest;
                    }
                } else {
                    if !pending.write {
                        state.write_gpr(pending.destination, input.data_read_data as u16);
                    }
                    state.retire(pending.retire_words);
                }
            }
            Phase::MultiplyWait => state.phase = Phase::MultiplyCommit,
            Phase::MultiplyCommit => {
                state.write_gpr(state.multiply_destination, state.multiply_result);
                state.retire(state.multiply_retire_words);
            }
            Phase::FpuExecute => state.execute_fpu_base(),
            Phase::FpuWriteLanes => {
                // Serial lane ALU: write the staged lane result while staging
                // the next lane from the wide register file reads.
                let a = usize::from(field(state.instruction, 4));
                let b = usize::from(field(state.instruction, 0));
                let function = field(state.instruction, 8);
                let unary = usize::from(field(state.instruction, 0));
                let lane = usize::from(state.fpu_step & 3);
                let left = state.fpu_operand_a;
                let value = match function {
                    8 => fix16_add(left, state.fpu_operand_b),
                    9 => fix16_sub(left, state.fpu_operand_b),
                    _ => match unary {
                        3 => fix16_abs(left),
                        4 => fix16_neg(left),
                        5 => fix16_floor(left),
                        6 => fix16_ceil(left),
                        7 => fix16_round(left),
                        8 => fix16_saturate01(left),
                        9 => fix16_sign(left),
                        _ => 0,
                    },
                };
                state.fpu_registers[a][lane] = value;
                if state.fpu_step < 3 {
                    let next = lane + 1;
                    state.fpu_operand_a = state.fpu_registers[a][next];
                    state.fpu_operand_b = state.fpu_registers[b][next];
                }
                if state.fpu_step == 3 {
                    state.fpu_step = 0;
                    state.phase = Phase::FpuCommit;
                } else {
                    state.fpu_step += 1;
                }
            }
            Phase::FpuUnaryDispatch => {
                let unary = field(state.instruction, 0);
                let domain_error = unary == 0 && state.fpu_operand_a == 0
                    || unary == 1 && state.fpu_operand_a <= 0;
                if domain_error {
                    state.fault(CPU_V3_FAULT_FPU_DOMAIN, state.fpu_fault_pc);
                } else {
                    state.phase = Phase::FpuRomNormalize;
                }
            }
            Phase::FpuGatherRead => {
                // Pack4 snapshots its four lane-x sources two vectors per
                // cycle, matching the two wide read ports.
                let b = usize::from(field(state.instruction, 0));
                if state.fpu_step == 0 {
                    state.fpu_memory_value[1] = state.fpu_registers[b + 1][0];
                    state.fpu_memory_value[2] = state.fpu_registers[b + 2][0];
                    state.fpu_step = 1;
                } else {
                    state.fpu_memory_value[3] = state.fpu_registers[b + 3][0];
                    state.fpu_step = 0;
                    state.phase = Phase::FpuGatherWrite;
                }
            }
            Phase::FpuGatherWrite => {
                let a = usize::from(field(state.instruction, 4));
                state.fpu_registers[a] = state.fpu_memory_value;
                state.phase = Phase::FpuCommit;
            }
            Phase::FpuScatter => {
                // One wide write per destination vector: lane zero carries the
                // selected source lane, the other lanes clear.
                let a = usize::from(field(state.instruction, 4));
                let lane = usize::from(state.fpu_step & 3);
                state.fpu_registers[a + lane] = [state.fpu_memory_value[lane], 0, 0, 0];
                if state.fpu_step == 3 {
                    state.fpu_step = 0;
                    state.phase = Phase::FpuCommit;
                } else {
                    state.fpu_step += 1;
                }
            }
            Phase::FpuTranspose => {
                // Steps 0-1 snapshot the remaining rows two per cycle; steps
                // 2-5 write one transposed row per cycle from the snapshot.
                let a = usize::from(field(state.instruction, 4));
                match state.fpu_step {
                    0 => {
                        state.fpu_transpose_rows[1] = state.fpu_registers[a + 1];
                        state.fpu_transpose_rows[2] = state.fpu_registers[a + 2];
                    }
                    1 => {
                        state.fpu_transpose_rows[3] = state.fpu_registers[a + 3];
                    }
                    _ => {
                        let row = usize::from(state.fpu_step - 2);
                        state.fpu_registers[a + row] = [
                            state.fpu_transpose_rows[0][row],
                            state.fpu_transpose_rows[1][row],
                            state.fpu_transpose_rows[2][row],
                            state.fpu_transpose_rows[3][row],
                        ];
                    }
                }
                if state.fpu_step == 5 {
                    state.fpu_step = 0;
                    state.phase = Phase::FpuCommit;
                } else {
                    state.fpu_step += 1;
                }
            }
            Phase::FpuMultiplyWait => state.phase = Phase::FpuMultiplySettle,
            Phase::FpuMultiplySettle => state.phase = Phase::FpuMultiplyCommit,
            Phase::FpuMultiplyCommit => {
                debug_assert_eq!(field(state.instruction, 8), 14);
                debug_assert_eq!(field(state.instruction, 0), 2);
                state.phase = Phase::FpuRomLookup;
            }
            Phase::FpuMultiplyPipeline => {
                let issue = state.fpu_step < 4;
                let issue_lane = usize::from(state.fpu_step & 3);
                let consume = state.fpu_mul_valid & 0b10 != 0;
                let consume_lane = usize::from(state.fpu_mul_tags[1]);
                let product = state.fpu_mul_products[1];
                let function = field(state.instruction, 8);
                if consume {
                    if function == 11 {
                        state.fpu_accumulator =
                            acc_saturate(i128::from(state.fpu_accumulator) + i128::from(product));
                    } else {
                        let a = usize::from(field(state.instruction, 4));
                        state.fpu_registers[a][consume_lane] = fix16_saturate(
                            round_shift_ties_even(product, crate::FIX16_FRACTION_BITS),
                        );
                    }
                }
                state.fpu_mul_valid = ((state.fpu_mul_valid & 1) << 1) | u8::from(issue);
                state.fpu_mul_tags[1] = state.fpu_mul_tags[0];
                state.fpu_mul_products[1] = state.fpu_mul_products[0];
                if issue {
                    state.fpu_mul_tags[0] = state.fpu_step;
                    state.fpu_mul_products[0] = state.fpu_product(issue_lane);
                    state.fpu_step += 1;
                }
                if consume && consume_lane == 3 {
                    state.fpu_step = 0;
                    state.fpu_mul_valid = 0;
                    state.phase = Phase::FpuCommit;
                }
            }
            Phase::FpuRomNormalize => state.phase = Phase::FpuRomAddress,
            Phase::FpuRomAddress => state.phase = Phase::FpuRomLookup,
            Phase::FpuRomLookup => state.phase = Phase::FpuRomWait,
            Phase::FpuRomWait => state.phase = Phase::FpuRomCommit,
            Phase::FpuRomCommit => {
                let operand = state.fpu_operand_a;
                match field(state.instruction, 0) {
                    0 => {
                        state.fpu_result = fix16_reciprocal(operand).expect("domain checked");
                        state.phase = Phase::FpuRomWrite;
                    }
                    1 => {
                        state.fpu_result = fix16_reciprocal_sqrt(operand).expect("domain checked");
                        state.phase = Phase::FpuRomWrite;
                    }
                    2 if state.fpu_rom_step == 0 => {
                        let (sin, cos) = fix16_sin_cos(operand);
                        state.fpu_rom_first = sin;
                        state.fpu_rom_second = cos;
                        state.fpu_rom_step = 1;
                        state.phase = Phase::FpuRomLookup;
                    }
                    2 => {
                        state.fpu_rom_step = 0;
                        state.phase = Phase::FpuRomWrite;
                    }
                    _ => unreachable!("only complex unary operations use the FPU ROM"),
                }
            }
            Phase::FpuRomWrite => {
                let destination = usize::from(field(state.instruction, 4));
                if field(state.instruction, 0) <= 1 {
                    // RCP/RSQRT write only lane zero.
                    state.fpu_registers[destination][0] = state.fpu_result;
                } else {
                    // SINCOS lands the whole vector in one wide write.
                    state.fpu_registers[destination] =
                        [state.fpu_rom_first, state.fpu_rom_second, 0, 0];
                }
                state.phase = Phase::FpuCommit;
            }
            Phase::FpuCommit => state.retire(state.fpu_retire_words),
            Phase::ResetClear => {
                let index = usize::from(state.fpu_clear_index);
                state.fpu_registers[index] = [0; 4];
                state.write_gpr(state.fpu_clear_index, 0);
                if state.fpu_clear_index == 15 {
                    state.phase = Phase::FetchRequest;
                } else {
                    state.fpu_clear_index += 1;
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
                    "__FPU_DSP_MULTIPLIER__",
                    &DspMulS18::verilog_identity().module_name(),
                )
                .replace("__FPU_ROM__", &FpuRom::verilog_identity().module_name())
                .replace(
                    "__FPU_REGISTER_RAM__",
                    &CpuV3FpuRegisterRam::verilog_identity().module_name(),
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
            VerilogDependency::new::<DspMulS18>("u_fpu_multiplier"),
            VerilogDependency::new::<FpuRom>("u_fpu_rom"),
            VerilogDependency::new::<CpuV3FpuRegisterRam>("u_fpu_register_ram"),
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
        |value| (value.high << 4) | (instruction & 15),
    )
}

fn is_prefix_consumer(instruction: u16) -> bool {
    match instruction >> 12 {
        8 | 9 => true,
        10 => !matches!((instruction >> 8) & 15, 5..=7),
        11 => matches!((instruction >> 8) & 15, 0..=5 | 8 | 9),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate as cpu_v3;
    use crate::rcc_backend::{self, CompilerOptions};
    use crate::{AluOp, ImmediateOp, Machine, RunOutcome, SpecialRegister, TestCondition};
    use digital_design_circuit::{build_circuit, Circuit};
    use digital_design_hardware::{ResourceAmount, ResourceKind, VerilogProject};
    use rcc::frontend::compile_program;
    use std::collections::HashMap;

    struct CoreRun {
        cycles: usize,
        halt_signal: u16,
        retired_words: u32,
        code_segment: u16,
        data_segment: u16,
        memory: HashMap<u32, u16>,
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

        for cycle in 0..maximum_cycles {
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
                    cycles: cycle,
                    halt_signal: value.halt_signal as u16,
                    retired_words: value.retired_words as u32,
                    code_segment: value.code_segment as u16,
                    data_segment: value.data_segment as u16,
                    memory,
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
        let mut oracle = Machine::default();
        oracle.load_program(0, &program).unwrap();
        let outcome = oracle.run(10_000).unwrap();
        let RunOutcome::Halted { signal, .. } = outcome else {
            panic!("oracle did not halt")
        };

        let mut memory = HashMap::new();
        load(&mut memory, 0, &program);
        // The compiler's static-data initialization runs from code, so the
        // external memory begins with the same zero-filled state as Machine.
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
            cpu_v3::alu(AluOp::Mul, 8, 6, 7),
            cpu_v3::move_register(3, 1),
            cpu_v3::set_less_than_signed(3, 2),
            cpu_v3::move_register(4, 1),
            cpu_v3::set_less_than_unsigned(4, 2),
            cpu_v3::population_count(5, 1),
            cpu_v3::immediate_unsigned(ImmediateOp::ShiftRightLogical, 1, 15),
            cpu_v3::alu(AluOp::Add, 0, 3, 4),
            cpu_v3::alu(AluOp::Add, 0, 0, 5),
            cpu_v3::alu(AluOp::Add, 0, 0, 8),
            cpu_v3::immediate_signed(ImmediateOp::CompareSigned, 0, 0),
            cpu_v3::branch(TestCondition::NotEqual, 1),
            cpu_v3::immediate_unsigned(ImmediateOp::LoadUnsigned, 0, 0),
            cpu_v3::halt(),
        ]);
        let mut oracle = Machine::default();
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
        let mut oracle = Machine::default();
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
    fn export_accounts_for_integer_and_fpu_multiplier_leaves() {
        let project = VerilogProject::generate::<CpuV3Core>().unwrap();
        assert_eq!(project.resource_claims.len(), 5);
        assert_eq!(
            project
                .resource_claims
                .iter()
                .filter(|claim| {
                    claim.resources == [ResourceAmount::new(ResourceKind::Multiplier18x18, 1)]
                })
                .count(),
            2
        );
        assert!(project
            .resource_claims
            .iter()
            .any(|claim| { claim.resources == [ResourceAmount::new(ResourceKind::Bsram18K, 1)] }));
        assert!(project.resource_claims.iter().any(|claim| {
            claim.resources
                == [ResourceAmount::new(
                    ResourceKind::SsramBit,
                    CPU_V3_FPU_REGISTER_PHYSICAL_BITS as u64,
                )]
        }));
        assert!(project.resource_claims.iter().any(|claim| {
            claim.resources
                == [ResourceAmount::new(
                    ResourceKind::SsramBit,
                    CPU_V3_GPR_PHYSICAL_BITS as u64,
                )]
        }));
    }

    #[test]
    fn emulator_matches_oracle_for_fix16_vector_datapath() {
        let mut program = vec![];
        program.extend(crate::load_immediate16(0, 384));
        program.extend(crate::load_immediate16(1, 512));
        program.extend([
            crate::fpu(crate::FpuOp::Load, 0, 0),
            crate::fpu(crate::FpuOp::Load, 1, 1),
            crate::fpu(crate::FpuOp::Add, 0, 1),
            crate::fpu(crate::FpuOp::MulScalar, 0, 1),
            crate::fpu(crate::FpuOp::Store, 0, 0),
            crate::halt(),
        ]);
        let mut oracle = Machine::default();
        oracle.load_program(0, &program).unwrap();
        let RunOutcome::Halted { signal, .. } = oracle.run(100).unwrap() else {
            panic!("oracle did not halt")
        };
        assert_eq!(signal, 1792);

        let mut memory = HashMap::new();
        load(&mut memory, 0, &program);
        let core = run_core(memory, 200);
        assert_eq!(core.halt_signal, signal);
        assert_eq!(core.retired_words as u64, oracle.retired_words());
    }

    #[test]
    fn fpu_pipelines_have_exact_blocking_latency() {
        fn program(operation: u16) -> Vec<u16> {
            let mut words = vec![];
            words.extend(crate::load_immediate16(0, 256));
            words.extend(crate::load_immediate16(1, 512));
            words.extend([
                crate::fpu(crate::FpuOp::Load, 0, 0),
                crate::fpu(crate::FpuOp::Load, 1, 1),
                operation,
                crate::fpu(crate::FpuOp::Store, 0, 0),
                crate::halt(),
            ]);
            words
        }

        let baseline = program(crate::move_register(15, 15));
        let mov = program(crate::fpu(crate::FpuOp::Move, 0, 1));
        let add = program(crate::fpu(crate::FpuOp::Add, 0, 1));
        let multiply = program(crate::fpu(crate::FpuOp::Mul, 0, 1));
        let pack = program(crate::fpu(crate::FpuOp::Pack4, 0, 1));
        let unpack = program(crate::fpu(crate::FpuOp::Unpack4, 0, 1));
        let transpose = program(crate::fpu(crate::FpuOp::Transpose4, 0, 0));
        let sincos = program(crate::fpu_unary(0, crate::FpuUnaryOp::SinCos));
        let reciprocal = program(crate::fpu_unary(0, crate::FpuUnaryOp::Reciprocal));
        let run = |words: &[u16]| {
            let mut memory = HashMap::new();
            load(&mut memory, 0, words);
            run_core(memory, 200)
        };
        let baseline = run(&baseline);
        let mov = run(&mov);
        let add = run(&add);
        let multiply = run(&multiply);
        let pack = run(&pack);
        let unpack = run(&unpack);
        let transpose = run(&transpose);
        let sincos = run(&sincos);
        let reciprocal = run(&reciprocal);

        // Wide-vector data movement commits one vec4 per phase; the serial
        // lane ALU and the ROM sequences are unchanged.
        assert_eq!(mov.cycles - baseline.cycles, 2);
        assert_eq!(add.cycles - baseline.cycles, 6);
        assert_eq!(multiply.cycles - baseline.cycles, 7);
        assert_eq!(pack.cycles - baseline.cycles, 5);
        assert_eq!(unpack.cycles - baseline.cycles, 6);
        assert_eq!(transpose.cycles - baseline.cycles, 8);
        assert_eq!(sincos.cycles - baseline.cycles, 12);
        assert_eq!(reciprocal.cycles - baseline.cycles, 9);
        assert_eq!(add.halt_signal, 768);
        assert_eq!(multiply.halt_signal, 512);
        assert_eq!(reciprocal.halt_signal, 256);
    }

    #[test]
    fn emulator_matches_oracle_for_fix16_register_file_hazards() {
        let mut program = vec![];
        for (register, value) in [(0, 1), (1, 2), (2, 3), (3, 4)] {
            program.extend(crate::load_immediate16(register, value));
            program.push(crate::fpu(crate::FpuOp::Load, register, register));
        }
        program.extend([
            crate::fpu(crate::FpuOp::Pack4, 1, 0),
            crate::fpu(crate::FpuOp::Unpack4, 0, 1),
        ]);
        for (fpr, gpr, address) in [
            (4, 1, 0x0100),
            (5, 2, 0x0104),
            (6, 3, 0x0108),
            (7, 4, 0x010c),
            (8, 5, 0x0110),
            (9, 6, 0x0114),
        ] {
            program.extend(crate::load_immediate16(gpr, address));
            program.push(crate::fpu(crate::FpuOp::Import4, fpr, gpr));
        }
        program.extend([
            crate::fpu(crate::FpuOp::Transpose4, 4, 0),
            crate::fpu(crate::FpuOp::Move, 10, 8),
            crate::fpu(crate::FpuOp::Mul, 10, 9),
        ]);
        for (fpr, gpr, address) in [
            (0, 7, 0x0120),
            (4, 8, 0x0124),
            (8, 9, 0x0128),
            (10, 10, 0x012c),
        ] {
            program.extend(crate::load_immediate16(gpr, address));
            program.push(crate::fpu(crate::FpuOp::Export4, fpr, gpr));
        }
        program.extend([
            crate::fpu(crate::FpuOp::Compare, 10, 8),
            crate::branch(crate::TestCondition::GreaterThan, 1),
            crate::immediate_unsigned(crate::ImmediateOp::LoadUnsigned, 0, 9),
            crate::halt(),
        ]);

        let mut oracle = Machine::default();
        oracle.load_program(0, &program).unwrap();
        for (index, value) in (1_u16..=16).enumerate() {
            oracle.physical_memory_mut()[0x0100 + index] = value;
        }
        for (index, value) in [256_u16, 512, (-256_i16) as u16, 128]
            .into_iter()
            .chain([512_u16, 128, (-512_i16) as u16, 512])
            .enumerate()
        {
            oracle.physical_memory_mut()[0x0110 + index] = value;
        }
        let RunOutcome::Halted { signal, .. } = oracle.run(1_000).unwrap() else {
            panic!("oracle did not halt")
        };

        let mut memory = HashMap::new();
        load(&mut memory, 0, &program);
        for (index, value) in (1_u16..=16).enumerate() {
            memory.insert(0x0100 + index as u32, value);
        }
        for (index, value) in [256_u16, 512, (-256_i16) as u16, 128]
            .into_iter()
            .chain([512_u16, 128, (-512_i16) as u16, 512])
            .enumerate()
        {
            memory.insert(0x0110 + index as u32, value);
        }
        let core = run_core(memory, 2_000);
        assert_eq!(core.halt_signal, signal);
        for address in 0x0120..0x0130 {
            assert_eq!(
                core.memory.get(&address).copied().unwrap_or(0),
                oracle.memory(address as u16),
                "exported FPR mismatch at {address:#06x}"
            );
        }
        assert_eq!(core.retired_words as u64, oracle.retired_words());
    }

    #[test]
    fn emulator_matches_oracle_for_fix16_memory_dot_acc_and_unary() {
        let mut program = vec![];
        program.extend(crate::load_immediate16(1, 0x0100));
        program.extend(crate::load_immediate16(2, 0x0104));
        program.extend([
            crate::fpu(crate::FpuOp::Import4, 0, 1),
            crate::fpu(crate::FpuOp::Import4, 1, 2),
            crate::fpu(crate::FpuOp::Dot4Acc, 0, 1),
            crate::fpu(crate::FpuOp::AccStore, 0, 0),
            crate::fpu(crate::FpuOp::Export4, 0, 2),
            crate::fpu_unary(0, crate::FpuUnaryOp::Abs),
            crate::fpu(crate::FpuOp::Store, 0, 0),
            crate::halt(),
        ]);
        let vector = [256_u16, 512, 768, 1024];
        let ones = [256_u16; 4];
        let mut oracle = Machine::default();
        oracle.load_program(0, &program).unwrap();
        for (lane, value) in vector.into_iter().chain(ones).enumerate() {
            oracle.physical_memory_mut()[0x0100 + lane] = value;
        }
        let RunOutcome::Halted { signal, .. } = oracle.run(200).unwrap() else {
            panic!("oracle did not halt")
        };
        assert_eq!(signal, 2560);

        let mut memory = HashMap::new();
        load(&mut memory, 0, &program);
        for (lane, value) in vector.into_iter().chain(ones).enumerate() {
            memory.insert(0x0100 + lane as u32, value);
        }
        let core = run_core(memory, 400);
        assert_eq!(core.halt_signal, signal);
        assert_eq!(core.retired_words as u64, oracle.retired_words());
    }

    #[test]
    fn emulator_matches_oracle_for_shared_rom_unary_operations() {
        let mut program = vec![];
        program.extend(crate::load_immediate16(0, 512));
        program.extend([
            crate::fpu(crate::FpuOp::Load, 0, 0),
            crate::fpu_unary(0, crate::FpuUnaryOp::Reciprocal),
            crate::fpu(crate::FpuOp::Store, 0, 0),
        ]);
        program.extend(crate::load_immediate16(1, 1024));
        program.extend([
            crate::fpu(crate::FpuOp::Load, 1, 1),
            crate::fpu_unary(1, crate::FpuUnaryOp::ReciprocalSqrt),
            crate::fpu(crate::FpuOp::Store, 1, 1),
            crate::fpu_unary(2, crate::FpuUnaryOp::Zero),
            crate::fpu_unary(2, crate::FpuUnaryOp::SinCos),
            crate::fpu(crate::FpuOp::Unpack4, 3, 2),
            crate::fpu(crate::FpuOp::Store, 0, 4),
            crate::halt(),
        ]);
        let mut oracle = Machine::default();
        oracle.load_program(0, &program).unwrap();
        let RunOutcome::Halted { signal, .. } = oracle.run(200).unwrap() else {
            panic!("oracle did not halt")
        };
        assert_eq!(signal, 256);
        assert_eq!(oracle.register(1), Some(128));

        let mut memory = HashMap::new();
        load(&mut memory, 0, &program);
        let core = run_core(memory, 400);
        assert_eq!(core.halt_signal, signal);
        assert_eq!(core.retired_words as u64, oracle.retired_words());
    }

    #[test]
    #[ignore = "explicit external simulation of the reusable CpuV3 core"]
    fn verify_verilog_with_iverilog() {
        digital_design_hardware::verify_verilog_with_iverilog::<CpuV3Core>().unwrap();
    }

    #[test]
    #[ignore = "explicit external simulation of the FPU register file"]
    fn verify_fpu_register_ram_with_iverilog() {
        digital_design_hardware::verify_verilog_with_iverilog::<CpuV3FpuRegisterRam>().unwrap();
    }

    #[test]
    #[ignore = "explicit external simulation of the scalar register file"]
    fn verify_gpr_ram_with_iverilog() {
        digital_design_hardware::verify_verilog_with_iverilog::<CpuV3GprRam>().unwrap();
    }
}
