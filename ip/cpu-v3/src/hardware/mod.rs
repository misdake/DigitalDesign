//! Reusable CpuV3 revision 0.7 processor core with physical-memory and device ports.

mod cache;
pub use cache::*;

use digital_design_circuit::{CircuitWires, Wire, Wires};
use digital_design_hardware::{
    HardwareIdentity, Module, ModuleIo, VerilogDependency, VerilogIdentity,
};
use digital_design_hardware_gowin::DspMulS18;
use std::cmp::Ordering;

use crate::{fix16_add, fix16_mul, fix16_sub, FpuVector};

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
    FpuMultiplyWait,
    FpuMultiplyCommit,
    FpuCommit,
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
    fpu_left: FpuVector,
    fpu_right: FpuVector,
    fpu_lane: u8,
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
            fpu_left: [0; 4],
            fpu_right: [0; 4],
            fpu_lane: 0,
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
                self.registers[usize::from(dst)] = match opcode {
                    0 => left.wrapping_add(right),
                    1 => left.wrapping_sub(right),
                    3 => left & right,
                    4 => left | right,
                    5 => left ^ right,
                    6 => left.wrapping_shl(u32::from(right & 15)),
                    7 => ((left as i16) >> u32::from(right & 15)) as u16,
                    _ => unreachable!(),
                };
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
                        self.registers[14] = next;
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
                    self.registers[usize::from(rhs)] = device_read_data;
                }
                self.retire(retire_words);
            }
            13 => {
                self.fpu_left = self.fpu_registers[usize::from(lhs)];
                self.fpu_right = self.fpu_registers[usize::from(rhs)];
                self.fpu_retire_words = retire_words;
                self.fpu_fault_pc = fault_pc;
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
        self.registers[usize::from(dst)] = result;
        self.retire(retire_words);
    }

    fn execute_control(&mut self, instruction: u16, retire_words: u8, fault_pc: u16) {
        let function = field(instruction, 8);
        let dst = field(instruction, 4);
        let src = field(instruction, 0);
        match function {
            0 => {
                self.registers[usize::from(dst)] =
                    self.registers[usize::from(src)].count_ones() as u16
            }
            1 => self.registers[usize::from(dst)] = self.registers[usize::from(src)],
            2 => self.registers[usize::from(dst)] = !self.registers[usize::from(src)],
            3 => self.registers[usize::from(dst)] = self.registers[usize::from(src)].wrapping_neg(),
            4 if dst == 0 => self.pc = self.registers[usize::from(src)],
            // JALR: the link field is architecturally fixed to r14.
            5 if dst == 14 => {
                let target = self.registers[usize::from(src)];
                self.registers[usize::from(dst)] = self.pc;
                self.pc = target;
            }
            6 => {
                self.registers[usize::from(dst)] =
                    sign_extend(self.registers[usize::from(src)] & 0xff, 8)
            }
            7 => {
                self.registers[usize::from(dst)] =
                    self.registers[usize::from(src)].leading_zeros() as u16
            }
            8 if dst == 0 && src == 0 => {
                self.retired_words = self.retired_words.wrapping_add(u32::from(retire_words));
                self.phase = Phase::Halted;
                return;
            }
            9 => {
                self.registers[usize::from(dst)] = u16::from(
                    (self.registers[usize::from(dst)] as i16)
                        < (self.registers[usize::from(src)] as i16),
                )
            }
            10 => {
                self.registers[usize::from(dst)] =
                    u16::from(self.registers[usize::from(dst)] < self.registers[usize::from(src)])
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
                self.registers[usize::from(dst)] = match src {
                    0 => self.code_segment,
                    1 => self.data_segment,
                    _ => {
                        self.fault(CPU_V3_FAULT_INVALID_INSTRUCTION, fault_pc);
                        return;
                    }
                }
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
            0 => self.fpu_registers[a] = [self.registers[b] as i16, 0, 0, 0],
            1 => self.registers[a] = self.fpu_registers[b][0] as u16,
            4 => self.fpu_registers[a] = self.fpu_registers[b],
            5 if b <= 12 => {
                let source = self.fpu_registers;
                self.fpu_registers[a] = std::array::from_fn(|lane| source[b + lane][0]);
            }
            6 if a <= 12 => {
                let source = self.fpu_registers[b];
                for (lane, value) in source.into_iter().enumerate() {
                    self.fpu_registers[a + lane] = [value, 0, 0, 0];
                }
            }
            7 if a <= 12 && b == 0 => {
                let source: [FpuVector; 4] = self.fpu_registers[a..a + 4]
                    .try_into()
                    .expect("validated four-register matrix");
                self.fpu_registers[a] = [source[0][0], source[1][0], source[2][0], source[3][0]];
                self.fpu_registers[a + 1] =
                    [source[0][1], source[1][1], source[2][1], source[3][1]];
                self.fpu_registers[a + 2] =
                    [source[0][2], source[1][2], source[2][2], source[3][2]];
                self.fpu_registers[a + 3] =
                    [source[0][3], source[1][3], source[2][3], source[3][3]];
            }
            8 => {
                self.fpu_registers[a] =
                    std::array::from_fn(|lane| fix16_add(self.fpu_left[lane], self.fpu_right[lane]))
            }
            9 => {
                self.fpu_registers[a] =
                    std::array::from_fn(|lane| fix16_sub(self.fpu_left[lane], self.fpu_right[lane]))
            }
            10 | 15 => {
                self.fpu_lane = 0;
                self.phase = Phase::FpuMultiplyWait;
                return;
            }
            _ => {
                self.fault(CPU_V3_FAULT_INVALID_INSTRUCTION, self.fpu_fault_pc);
                return;
            }
        }
        self.phase = Phase::FpuCommit;
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
                instruction_response_ready: state.phase == Phase::FetchResponse,
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
            return;
        }
        match state.phase {
            Phase::FetchRequest if input.instruction_request_ready => {
                state.phase = Phase::FetchResponse;
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
                    state.fault(CPU_V3_FAULT_DATA_MEMORY, pending.fault_pc);
                } else {
                    if !pending.write {
                        state.registers[usize::from(pending.destination)] =
                            input.data_read_data as u16;
                    }
                    state.retire(pending.retire_words);
                }
            }
            Phase::MultiplyWait => state.phase = Phase::MultiplyCommit,
            Phase::MultiplyCommit => {
                state.registers[usize::from(state.multiply_destination)] = state.multiply_result;
                state.retire(state.multiply_retire_words);
            }
            Phase::FpuExecute => state.execute_fpu_base(),
            Phase::FpuMultiplyWait => state.phase = Phase::FpuMultiplyCommit,
            Phase::FpuMultiplyCommit => {
                let destination = usize::from(field(state.instruction, 4));
                let lane = usize::from(state.fpu_lane);
                let scalar = field(state.instruction, 8) == 15;
                state.fpu_registers[destination][lane] = fix16_mul(
                    state.fpu_left[lane],
                    state.fpu_right[if scalar { 0 } else { lane }],
                );
                if state.fpu_lane == 3 {
                    state.phase = Phase::FpuCommit;
                } else {
                    state.fpu_lane += 1;
                    state.phase = Phase::FpuMultiplyWait;
                }
            }
            Phase::FpuCommit => state.retire(state.fpu_retire_words),
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
                ),
        )
    }

    fn verilog_dependencies() -> Vec<VerilogDependency> {
        vec![
            VerilogDependency::new::<DspMulS18>("u_multiplier"),
            VerilogDependency::new::<DspMulS18>("u_fpu_multiplier"),
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

        for _ in 0..maximum_cycles {
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
        assert_eq!(project.resource_claims.len(), 2);
        assert!(project.resource_claims.iter().all(|claim| {
            claim.resources == [ResourceAmount::new(ResourceKind::Multiplier18x18, 1)]
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
    #[ignore = "explicit external simulation of the reusable CpuV3 core"]
    fn verify_verilog_with_iverilog() {
        digital_design_hardware::verify_verilog_with_iverilog::<CpuV3Core>().unwrap();
    }
}
