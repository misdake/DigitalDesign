use digital_design_circuit::{CircuitWires, Wire, Wires};
use digital_design_hardware::{
    resources::components::SsramBits, HardwareIdentity, Module, ModuleIo, TargetResourceRequest,
    VerilogDependency, VerilogIdentity,
};
use digital_design_hardware_gowin::DspMulS18;
use std::cmp::Ordering;

use crate::SignalEvent;

use super::fpu::{encoding, CpuV3Fpu, CpuV3FpuInputValue, CpuV3FpuOutputValue, CpuV3FpuState};

pub const CPU_V3_FAULT_INVALID_INSTRUCTION: u8 = 1;
pub const CPU_V3_FAULT_INSTRUCTION_MEMORY: u8 = 3;
pub const CPU_V3_FAULT_DATA_MEMORY: u8 = 4;

// AUX kind-00 integer-bridge subops (design section 10.2). The core performs
// these moves through the FPU ext read/write ports because the F register file
// is only reachable there; the numeric conversions are `I16TOF`/`FTOI16`.
const AUX_ILO2F: u8 = 0x02;
const AUX_IHI2F: u8 = 0x03;
const AUX_FLO2I: u8 = 0x04;
const AUX_FHI2I: u8 = 0x05;
const AUX_I16TOF: u8 = 0x06;
const AUX_FTOI16: u8 = 0x07;

/// Is `subop` an AUX kind-00 integer bridge?
fn aux_bridge_subop(subop: u8) -> bool {
    (AUX_ILO2F..=AUX_FTOI16).contains(&subop)
}

/// The bridge's F read is `Fd` for the two low/high merges and `Fa` for the
/// extractions.
fn aux_bridge_reads_fd(subop: u8) -> bool {
    matches!(subop, AUX_ILO2F | AUX_IHI2F | AUX_I16TOF)
}

/// Does the bridge write an F register (rather than a GPR)?
fn aux_bridge_writes_f(subop: u8) -> bool {
    matches!(subop, AUX_ILO2F | AUX_IHI2F | AUX_I16TOF)
}

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

impl HardwareIdentity for CpuV3Core {
    const TARGET_RESOURCE_LEAF: bool = false;

    fn verilog_identity() -> VerilogIdentity {
        VerilogIdentity::new("CpuV3Core").namespace(["components", "cpu", "cpu_v3"])
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Phase {
    FetchRequest,
    FetchResponse,
    Execute,
    DataRequest,
    DataResponse,
    AsyncStoreWait,
    MultiplyWait,
    MultiplyCommit,
    /// FPU v2: fetch the second instruction word through the instruction port.
    Fpu2Word1,
    /// FPU v2: scalar ops wait out the unit; memory ops wait for the async
    /// store buffer to drain before taking the data port.
    Fpu2Exec,
    /// FPU v2: capture an FST source vector into the early-release store
    /// buffer, one F register per beat through the FPU ext read port.
    Fpu2Capture,
    /// FPU v2: AUX kind-00 integer bridge. Read the source F register (when the
    /// subop needs it) through the ext read port and wait for the unit to
    /// finish the pair.
    Fpu2AuxRead,
    /// FPU v2: apply the AUX bridge (write an F register through the ext write
    /// port or a GPR) and retire.
    Fpu2AuxApply,
    Fpu2MemRequest,
    Fpu2MemResponse,
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
pub(crate) struct PendingData {
    pub(crate) write: bool,
    pub(crate) address: u32,
    pub(crate) write_data: u16,
    pub(crate) destination: u8,
    pub(crate) retire_words: u8,
    pub(crate) fault_pc: u16,
}

#[derive(Clone, Copy, Default)]
pub(crate) struct AsyncStore {
    pub(crate) valid: bool,
    pub(crate) issued: bool,
    pub(crate) address: u32,
    pub(crate) write_data: u16,
    pub(crate) fault_pc: u16,
}

pub struct CpuV3CoreState {
    pub(crate) registers: [u16; 16],
    pub(crate) gpr_write_enable: bool,
    gpr_write_address: u8,
    pub(crate) gpr_write_data: u16,
    pc: u16,
    code_segment: u16,
    data_segment: u16,
    prefix: Option<Prefix>,
    /// Transient result of the last CMP-class instruction; mirrors the
    /// architectural `CpuV3Sim::pending_test` (consumed by conditional
    /// branches, expired by any other retired non-prefix instruction).
    pending_test: Option<Ordering>,
    pub(crate) phase: Phase,
    pub(crate) instruction: u16,
    instruction_pc: u16,
    pub(crate) pending_data: PendingData,
    pub(crate) async_store: AsyncStore,
    multiply_destination: u8,
    multiply_product: u32,
    multiply_shift: u8,
    multiply_retire_words: u8,
    /// Reset walks the scalar register file back to zero one word per cycle.
    clear_index: u8,
    pub(crate) retired_words: u32,
    fault_code: u8,
    fault_pc: u16,
    /// The FPU v2 unit (frontend + register file + scalar path).
    fpu: CpuV3FpuState,
    fpu2_word_valid: bool,
    fpu2_word: u16,
    fpu2_abort: bool,
    fpu2_word0: u16,
    fpu2_subop: u8,
    fpu2_fd: u8,
    fpu2_fa: u8,
    fpu2_is_memory: bool,
    fpu2_write_back: bool,
    fpu2_aux_data: u32,
    /// Half-beat counter across the whole vector memory transaction (2*len
    /// beats); bit 0 selects low/high, the upper bits select the F register.
    fpu2_beat: u8,
    /// Register count of the current memory access (1 scalar, 2..4 vector),
    /// derived from mode[1:0]; reserved mode[3:2] forces the scalar length.
    fpu2_len: u8,
    fpu2_address: u32,
    fpu2_low: u16,
    fpu2_seen_complete: bool,
    fpu2_fault_pc: u16,
    /// FST early-release store buffer (FPU design section 18): up to four
    /// captured 32-bit F registers, the address and fault pc of the pending
    /// store, and the remaining 16-bit word count/index draining through the
    /// single-entry async store channel.
    fpu2_buf: [u32; 4],
    fpu2_pending: u8,
    fpu2_pending_index: u8,
    fpu2_store_address: u32,
    fpu2_store_fault_pc: u16,
    /// The architectural halt value, latched at the HALT retire edge like a
    /// register-read (mirrors the RTL's registered `halt_signal`).
    pub(crate) halt_signal: u16,
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
            fpu: CpuV3FpuState::default(),
            fpu2_word_valid: false,
            fpu2_word: 0,
            fpu2_abort: false,
            fpu2_word0: 0,
            fpu2_subop: 0,
            fpu2_fd: 0,
            fpu2_fa: 0,
            fpu2_is_memory: false,
            fpu2_write_back: false,
            fpu2_aux_data: 0,
            fpu2_beat: 0,
            fpu2_len: 1,
            fpu2_address: 0,
            fpu2_low: 0,
            fpu2_seen_complete: false,
            fpu2_fault_pc: 0,
            fpu2_buf: [0; 4],
            fpu2_pending: 0,
            fpu2_pending_index: 0,
            fpu2_store_address: 0,
            fpu2_store_fault_pc: 0,
        }
    }
}

impl CpuV3CoreState {
    /// This cycle's FPU v2 unit inputs, mirroring the RTL wiring: the
    /// registered word stream plus the combinational ext channel (owned by
    /// the core during the FPU memory states).
    fn fpu_input(
        &self,
        data_response_valid: bool,
        data_error: bool,
        data_read_data: u16,
    ) -> CpuV3FpuInputValue {
        let aux_apply = self.phase == Phase::Fpu2AuxApply;
        let aux_write_f = aux_apply && aux_bridge_writes_f(self.fpu2_subop);
        let ext_access = (self.phase == Phase::Fpu2Exec && self.fpu2_is_memory)
            || self.phase == Phase::Fpu2Capture
            || self.phase == Phase::Fpu2AuxRead
            || aux_write_f
            || matches!(self.phase, Phase::Fpu2MemRequest | Phase::Fpu2MemResponse);
        let ext_write_enable = (self.phase == Phase::Fpu2MemResponse
            && !self.fpu2_write_back
            && self.fpu2_beat & 1 == 1
            && data_response_valid
            && !data_error)
            || aux_write_f;
        // The register index advances once per two beats (one F register), so
        // beat >> 1 selects Fd+i for FLD and Fa+i for FST, matching the RTL.
        // During capture the synchronous RF read is pipelined: present the next
        // register (beat + 1) while the current one is captured.
        let ext_read_index = if self.phase == Phase::Fpu2Capture {
            u64::from(self.fpu2_beat) + 1
        } else {
            u64::from(self.fpu2_beat >> 1)
        };
        let ext_read_address = if self.phase == Phase::Fpu2AuxRead {
            let index = if aux_bridge_reads_fd(self.fpu2_subop) {
                self.fpu2_fd
            } else {
                self.fpu2_fa
            };
            u64::from(index)
        } else {
            u64::from(self.fpu2_fa) + ext_read_index
        };
        let gpr_x = u32::from(self.registers[usize::from(encoding::aux_x(self.fpu2_word0))]);
        let memory_write = (u32::from(data_read_data) << 16) | u32::from(self.fpu2_low);
        let ext_write_data = if aux_apply {
            match self.fpu2_subop {
                AUX_ILO2F => (self.fpu2_aux_data & 0xffff_0000) | gpr_x,
                AUX_IHI2F => (self.fpu2_aux_data & 0xffff) | (gpr_x << 16),
                AUX_I16TOF => gpr_x << 16,
                _ => memory_write,
            }
        } else {
            memory_write
        };
        CpuV3FpuInputValue {
            abort: self.fpu2_abort,
            word_valid: self.fpu2_word_valid,
            word: u64::from(self.fpu2_word),
            ext_access,
            ext_write_enable,
            ext_write_address: u64::from(self.fpu2_fd) + (u64::from(self.fpu2_beat) >> 1),
            ext_write_data: u64::from(ext_write_data),
            ext_read_address,
        }
    }

    /// Drives the FPU v2 unit with this cycle's inputs, samples its
    /// combinational outputs, and clocks its registers. The returned outputs
    /// are pre-edge, mirroring the RTL's nonblocking semantics; the one-beat
    /// `word_valid` / `abort` strobes are cleared after the edge so the unit
    /// sees them for exactly one cycle.
    fn tick_fpu2(
        &mut self,
        data_response_valid: bool,
        data_error: bool,
        data_read_data: u16,
    ) -> CpuV3FpuOutputValue {
        let fpu_input = self.fpu_input(data_response_valid, data_error, data_read_data);
        let fpu_out = self.fpu.comb(&fpu_input);
        self.fpu.tick(&fpu_input);
        self.fpu2_word_valid = false;
        self.fpu2_abort = false;
        fpu_out
    }

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

    pub(crate) fn execute(&mut self, device_read_data: u16) {
        let instruction = self.instruction;
        let opcode = encoding::opcode(instruction);
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
            // FPU v2 two-word instruction (VECTOR / SCALAR / AUX): word0 goes
            // to the unit now, word1 is fetched through the instruction port.
            // FPU instructions never consume PFX12; a pending prefix already
            // retired separately above.
            encoding::OPCODE_VECTOR..=encoding::OPCODE_AUX => {
                self.fpu2_word = instruction;
                self.fpu2_word_valid = true;
                self.fpu2_word0 = instruction;
                self.fpu2_fault_pc = self.instruction_pc;
                self.fpu2_seen_complete = false;
                self.fpu2_is_memory = false;
                self.fpu2_write_back = false;
                self.phase = Phase::Fpu2Word1;
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
        // FPU v2: the unit's combinational read data feeds FST write beats.
        let fpu_input = state.fpu_input(
            input.data_response_valid,
            input.data_error,
            input.data_read_data as u16,
        );
        let fpu_out = state.fpu.comb(&fpu_input);
        let fpu_mem_active = matches!(state.phase, Phase::Fpu2MemRequest | Phase::Fpu2MemResponse);
        let device_instruction = state.phase == Phase::Execute && state.instruction >> 12 == 0x7;
        let device_field = field(state.instruction, 8);
        let device_register = field(state.instruction, 0);
        output.drive(
            circuit,
            &CpuV3CoreOutputValue {
                instruction_request_valid: !input.hold
                    && (state.phase == Phase::FetchRequest
                        || state.phase == Phase::Fpu2Word1
                        || execute_pipelineable),
                instruction_address: u64::from(physical_address(state.code_segment, state.pc)),
                instruction_response_ready: !input.hold
                    && (matches!(
                        state.phase,
                        Phase::FetchRequest | Phase::FetchResponse | Phase::Fpu2Word1
                    ) || execute_pipelineable),
                data_request_valid: !input.hold
                    && ((store.valid && !store.issued)
                        || state.phase == Phase::DataRequest
                        || state.phase == Phase::Fpu2MemRequest),
                data_write: store.valid
                    || if fpu_mem_active {
                        state.fpu2_write_back
                    } else {
                        pending.write
                    },
                data_address: u64::from(if store.valid {
                    store.address
                } else if fpu_mem_active {
                    state.fpu2_address + u32::from(state.fpu2_beat)
                } else {
                    pending.address
                }),
                data_write_data: u64::from(if store.valid {
                    store.write_data
                } else if fpu_mem_active {
                    // FST beats stream the low then the high half of the F
                    // register; FLD never drives write data.
                    if state.fpu2_beat & 1 == 1 {
                        (fpu_out.ext_read_data >> 16) as u16
                    } else {
                        fpu_out.ext_read_data as u16
                    }
                } else {
                    pending.write_data
                }),
                data_response_ready: !input.hold
                    && ((store.valid && store.issued)
                        || state.phase == Phase::DataResponse
                        || state.phase == Phase::Fpu2MemResponse),
                device_index: u64::from(device_field & 7),
                device_channel: u64::from(field(state.instruction, 4)),
                device_read_enable: !input.hold && device_instruction && device_field & 8 == 0,
                device_write_enable: !input.hold && device_instruction && device_field & 8 != 0,
                device_write_data: u64::from(state.registers[usize::from(device_register)]),
                halted: state.phase == Phase::Halted && !store.valid && state.fpu2_pending == 0,
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
        // Pre-edge FST drain state for the same nonblocking reason: the capture
        // that retires on this edge must not make the refill fire early.
        let fpu2_pending_was = state.fpu2_pending;
        let fpu2_pending_index_was = state.fpu2_pending_index;
        let fpu2_word_reg = ((fpu2_pending_index_was >> 1) & 0x3) as usize;
        let fpu2_word_data = state.fpu2_buf[fpu2_word_reg];
        let fpu2_word_half = if fpu2_pending_index_was & 1 == 1 {
            (fpu2_word_data >> 16) as u16
        } else {
            fpu2_word_data as u16
        };
        let async_store_completing = async_store_was_valid
            && async_store_was_issued
            && input.data_response_valid
            && !input.data_error;
        let async_store_free = !async_store_was_valid || async_store_completing;
        let cpu_mem_active = matches!(state.phase, Phase::DataRequest | Phase::DataResponse);
        let fpu_mem_active = matches!(state.phase, Phase::Fpu2MemRequest | Phase::Fpu2MemResponse);
        let cpu_store_enqueue = state.phase == Phase::Execute
            && (state.instruction >> 12) == 9
            && !async_store_was_valid;
        let fpu_refill = fpu2_pending_was != 0
            && async_store_free
            && !cpu_store_enqueue
            && !cpu_mem_active
            && !fpu_mem_active;
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
        // The buffer drain is applied only after the phase logic below: an
        // instruction that retires on the same edge must still observe the
        // pre-edge `async_store.valid` (the RTL reads the nonblocking
        // `async_store_valid`), otherwise a store behind a draining buffer
        // would enqueue a beat early instead of waiting in AsyncStoreWait.
        // Drive the FPU v2 unit with this cycle's inputs and clock it. The
        // combinational outputs seen by the phase logic below are pre-edge,
        // mirroring the RTL's nonblocking semantics.
        let fpu_out = state.tick_fpu2(
            input.data_response_valid,
            input.data_error,
            input.data_read_data as u16,
        );
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
            Phase::AsyncStoreWait if !async_store_was_valid && fpu2_pending_was == 0 => {
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
            // ---- FPU v2 handshake phase arms ----
            // The unit was driven and clocked by `tick_fpu2` above; these arms
            // interpret its outputs and advance the two-word fetch plus the
            // FLD/FST memory beat sequence. They share the pre-edge locals
            // sampled at the top of this function (`fpu2_pending_was`,
            // `async_store_was_valid`, `fpu_out`, ...), so they stay inline
            // rather than moving across a helper boundary.
            Phase::Fpu2Word1 => {
                if input.instruction_response_valid {
                    if input.instruction_error {
                        state.fpu2_abort = true;
                        state.fault(CPU_V3_FAULT_INSTRUCTION_MEMORY, state.pc);
                    } else {
                        let word1 = input.instruction_data as u16;
                        state.fpu2_word = word1;
                        state.fpu2_word_valid = true;
                        state.pc = state.pc.wrapping_add(1);
                        state.fpu2_subop = encoding::scalar_subop(word1);
                        state.fpu2_fd = encoding::word1_fd(word1);
                        state.fpu2_fa = encoding::aux_fa(state.fpu2_word0);
                        // AUX kind 00, subops FLD (0x00) / FST (0x01).
                        state.fpu2_is_memory = encoding::opcode(state.instruction)
                            == encoding::OPCODE_AUX
                            && encoding::aux_kind(state.fpu2_word0) == 0
                            && matches!(state.fpu2_subop, encoding::FLD | encoding::FST);
                        state.fpu2_write_back = state.fpu2_subop == encoding::FST;
                        let is_aux_bridge = encoding::opcode(state.instruction)
                            == encoding::OPCODE_AUX
                            && encoding::aux_kind(state.fpu2_word0) == 0
                            && aux_bridge_subop(state.fpu2_subop);
                        // mode[1:0]: 00 scalar, 01 vec2, 10 vec3, 11 vec4, so
                        // len = mode[1:0] + 1. Reserved mode[3:2] != 00 is
                        // defined to behave as the scalar form (len 1).
                        let mode = encoding::word1_mode(word1);
                        state.fpu2_len = if mode & 0xC != 0 { 1 } else { (mode & 0x3) + 1 };
                        state.fpu2_address = physical_address(
                            state.data_segment,
                            state.registers[usize::from(encoding::aux_x(state.fpu2_word0))],
                        );
                        state.fpu2_beat = 0;
                        state.phase = if is_aux_bridge {
                            Phase::Fpu2AuxRead
                        } else {
                            Phase::Fpu2Exec
                        };
                    }
                }
            }
            Phase::Fpu2Exec => {
                let seen_complete = state.fpu2_seen_complete;
                if fpu_out.instr_complete {
                    state.fpu2_seen_complete = true;
                }
                if state.fpu2_is_memory {
                    if fpu2_pending_was == 0 && !async_store_was_valid {
                        state.fpu2_beat = 0;
                        state.phase = if state.fpu2_write_back {
                            Phase::Fpu2Capture
                        } else {
                            Phase::Fpu2MemRequest
                        };
                    }
                } else if seen_complete && !fpu_out.busy {
                    // A scalar `CMP` publishes its signed Q16.16 ordering into
                    // the core's pending test, exactly like CMPS/CMPU. The
                    // flags were registered by the scalar path when the ALU
                    // ran, so they are valid at this retire edge; the following
                    // conditional branch/conditional move consumes them.
                    if encoding::opcode(state.fpu2_word0) == encoding::OPCODE_SCALAR
                        && state.fpu2_subop == encoding::CMP
                    {
                        state.pending_test = Some(if fpu_out.flag_lt {
                            Ordering::Less
                        } else if fpu_out.flag_eq {
                            Ordering::Equal
                        } else {
                            Ordering::Greater
                        });
                    }
                    state.retire(2);
                    state.phase = Phase::FetchRequest;
                }
            }
            Phase::Fpu2AuxRead => {
                // The F register was addressed through the ext read port this
                // cycle; the synchronous RF data lands next cycle. Wait for the
                // unit's `instr_complete` strobe (sampled pre-edge, like the
                // RTL's registered `fpu2_seen_complete`), then latch the read
                // value.
                let seen_complete = state.fpu2_seen_complete;
                if fpu_out.instr_complete {
                    state.fpu2_seen_complete = true;
                }
                if seen_complete {
                    state.fpu2_aux_data = fpu_out.ext_read_data as u32;
                    state.phase = Phase::Fpu2AuxApply;
                }
            }
            Phase::Fpu2AuxApply => {
                // F writes were driven combinationally through the ext write
                // port this cycle; the GPR-writing bridges stage their result
                // through the normal synchronous GPR write.
                let x = encoding::aux_x(state.fpu2_word0);
                match state.fpu2_subop {
                    AUX_FLO2I => state.write_gpr(x, (state.fpu2_aux_data & 0xffff) as u16),
                    AUX_FHI2I => state.write_gpr(x, (state.fpu2_aux_data >> 16) as u16),
                    AUX_FTOI16 => {
                        state.write_gpr(x, crate::fix32_to_i16(state.fpu2_aux_data as i32) as u16)
                    }
                    _ => {}
                }
                state.retire(2);
            }
            Phase::Fpu2Capture => {
                // The F value indexed by fpu2_beat was requested on the previous
                // beat; capture it while fpu_input presents beat + 1.
                let index = state.fpu2_beat as usize;
                if index < state.fpu2_buf.len() {
                    state.fpu2_buf[index] = fpu_out.ext_read_data as u32;
                }
                if state.fpu2_beat == state.fpu2_len - 1 {
                    state.fpu2_store_address = state.fpu2_address;
                    state.fpu2_store_fault_pc = state.fpu2_fault_pc;
                    state.fpu2_pending = state.fpu2_len * 2;
                    state.fpu2_pending_index = 0;
                    state.retire(2);
                } else {
                    state.fpu2_beat += 1;
                }
            }
            Phase::Fpu2MemRequest if input.data_request_ready => {
                state.phase = Phase::Fpu2MemResponse;
            }
            Phase::Fpu2MemResponse if input.data_response_valid => {
                if input.data_error {
                    state.fpu2_abort = true;
                    let fault_pc = state.fpu2_fault_pc;
                    state.fault(CPU_V3_FAULT_DATA_MEMORY, fault_pc);
                } else if state.fpu2_beat & 1 == 0 {
                    // Low half of an F register: FLD keeps it for the write on
                    // the following high beat.
                    state.fpu2_low = input.data_read_data as u16;
                    state.fpu2_beat += 1;
                    state.phase = Phase::Fpu2MemRequest;
                } else if state.fpu2_beat + 1 == state.fpu2_len * 2 {
                    // Last high half: the whole vector retires.
                    state.retire(2);
                } else {
                    // High half of a non-final register; the next register's
                    // low half begins.
                    state.fpu2_beat += 1;
                    state.phase = Phase::Fpu2MemRequest;
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
        // Apply the completed-async-store drain after the phase logic, exactly
        // like the RTL's nonblocking `async_store_valid <= 0` assignment: a
        // store or load retiring on this edge saw the pre-edge valid bit.
        if async_store_was_valid && async_store_was_issued && input.data_response_valid {
            state.async_store = AsyncStore::default();
        }
        // FST early-release drain refill, mirroring the RTL's assignment that
        // follows the phase case: a free (or completing) channel takes the
        // next pending word, keeping the drain gapless. A CPU store enqueuing
        // in Execute is excluded so it wins a genuinely empty channel.
        if fpu_refill {
            state.async_store = AsyncStore {
                valid: true,
                issued: false,
                address: state
                    .fpu2_store_address
                    .wrapping_add(u32::from(fpu2_pending_index_was)),
                write_data: fpu2_word_half,
                fault_pc: state.fpu2_store_fault_pc,
            };
            state.fpu2_pending = fpu2_pending_was - 1;
            state.fpu2_pending_index = fpu2_pending_index_was + 1;
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
            VerilogDependency::new::<CpuV3Fpu>("u_fpu"),
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
