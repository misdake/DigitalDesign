//! Small architectural interpreter used as the CpuV3 correctness oracle.

use super::encoding::{
    fpu_aux_fa, fpu_aux_field_error, fpu_aux_x, fpu_fa, fpu_fb, fpu_fd, fpu_mode,
    fpu_scalar_field_error, fpu_scalar_subop_field, fpu_vector_field_error, fpu_vector_len_field,
    fpu_vector_mode, fpu_vector_subop_field, is_prefix_consumer, sign_extend, SpecialRegister,
    Word, CONSTANT_TABLE, LINK_REGISTER,
};
use super::{
    fix32_abs, fix32_accumulate_product, fix32_add, fix32_ceil, fix32_compare, fix32_floor,
    fix32_from_acc, fix32_from_i16, fix32_mul, fix32_neg, fix32_round, fix32_sub, fix32_to_i16,
    fix32_trunc, rcp_q16, rsqrt_q16, sincos_q16, FpuAuxKind, FpuAuxSubop, FpuDotStride, FpuOpcode,
    FpuScalarSubop, FpuSinCosMode, FpuVectorLength, FpuVectorSubop, PhysicalWordAddress,
    FPU_REGISTER_COUNT,
};
use std::cmp::Ordering;

/// Fitted 8-MiB SDRAM on the initial Tang Nano 20K target.
pub const DEFAULT_PHYSICAL_MEMORY_WORDS: usize = 1 << 22;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FaultKind {
    InvalidInstruction,
    PhysicalAddressOutOfRange { address: PhysicalWordAddress },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Fault {
    pub kind: FaultKind,
    pub address: Word,
    pub instruction: Word,
}

/// A non-halting `SIGNAL` (types 1..=15) observed at its retirement edge.
/// Models expose it as an event; hardware retires these types as a NOP.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SignalEvent {
    pub signal_type: u8,
    pub value: Word,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StepOutcome {
    Running,
    /// A nonzero `SIGNAL` retired. `run()` ignores the event and continues.
    Signaled(SignalEvent),
    Halted {
        signal: Word,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Prefix {
    address: Word,
    payload: Word,
}

/// A device reached exclusively through DEVRECV and DEVSEND. `memory` is the
/// complete physical memory, so DMA-style devices can move data.
pub trait Device {
    fn read(&mut self, memory: &mut [Word], channel: u8) -> Word;
    fn write(&mut self, memory: &mut [Word], channel: u8, value: Word);
    /// Downcast support for test assertions on attached models.
    fn as_any(&self) -> &dyn std::any::Any;
}

pub struct CpuV3Sim {
    registers: [Word; 16],
    fpu_registers: [i32; FPU_REGISTER_COUNT],
    fpu_accumulator: i64,
    memory: Box<[Word]>,
    devices: [Option<Box<dyn Device>>; 8],
    /// Optional BSRAM boot-window image: instruction fetches from the lowest
    /// physical words read this image instead of main memory, matching the
    /// hardware split between the boot BSRAM (instruction side) and SDRAM
    /// (data side). Without it, memory is fully unified.
    boot_window: Option<Box<[Word]>>,
    pc: Word,
    code_segment: Word,
    data_segment: Word,
    prefix: Option<Prefix>,
    /// Transient result of the last CMP-class instruction, consumed by the
    /// next conditional branch and expired by any other retired
    /// non-prefix instruction (prefixes are transparent to it).
    pending_test: Option<Ordering>,
    retired_words: u64,
    halted: bool,
    /// Halt signal latched at the SIGNAL-type-0 retirement edge.
    halt_signal: Word,
}

impl Default for CpuV3Sim {
    fn default() -> Self {
        Self::with_physical_memory_words(DEFAULT_PHYSICAL_MEMORY_WORDS)
    }
}

impl CpuV3Sim {
    pub fn with_physical_memory_words(words: usize) -> Self {
        assert!(
            words > 0,
            "CpuV3 physical memory must contain at least one word"
        );
        assert!(
            words <= (u32::MAX as usize) + 1,
            "CpuV3 physical memory exceeds the architectural word address space"
        );
        Self {
            registers: [0; 16],
            fpu_registers: [0; FPU_REGISTER_COUNT],
            fpu_accumulator: 0,
            memory: vec![0; words].into_boxed_slice(),
            devices: std::array::from_fn(|_| None),
            boot_window: None,
            pc: 0,
            code_segment: 0,
            data_segment: 0,
            prefix: None,
            pending_test: None,
            retired_words: 0,
            halted: false,
            halt_signal: 0,
        }
    }
}

impl CpuV3Sim {
    pub fn load_program(&mut self, base: Word, words: &[Word]) -> Result<(), ProgramLoadError> {
        self.load_physical(PhysicalWordAddress::from(base), words)
    }

    pub fn load_segment(
        &mut self,
        segment: Word,
        offset: Word,
        words: &[Word],
    ) -> Result<(), ProgramLoadError> {
        self.load_physical(
            PhysicalWordAddress::from_segment_offset(segment, offset),
            words,
        )
    }

    pub fn load_physical(
        &mut self,
        base: PhysicalWordAddress,
        words: &[Word],
    ) -> Result<(), ProgramLoadError> {
        let start = base.get() as usize;
        let end = start
            .checked_add(words.len())
            .filter(|end| *end <= self.memory.len())
            .ok_or(ProgramLoadError {
                base,
                words: words.len(),
            })?;
        self.memory[start..end].copy_from_slice(words);
        Ok(())
    }

    pub fn registers(&self) -> &[Word; 16] {
        &self.registers
    }

    pub fn register(&self, index: u8) -> Option<Word> {
        self.registers.get(usize::from(index)).copied()
    }

    pub fn fpu_registers(&self) -> &[i32; FPU_REGISTER_COUNT] {
        &self.fpu_registers
    }

    /// One Q16.16 scalar F register, or `None` when `index` is outside
    /// `F0..F63`.
    pub fn fpu_register(&self, index: u8) -> Option<i32> {
        self.fpu_registers.get(usize::from(index)).copied()
    }

    pub fn fpu_accumulator(&self) -> i64 {
        self.fpu_accumulator
    }

    pub fn memory(&self, address: Word) -> Word {
        self.physical_memory(address.into())
    }

    pub fn physical_memory(&self, address: PhysicalWordAddress) -> Word {
        self.memory[address.get() as usize]
    }

    pub fn physical_memory_words(&self) -> usize {
        self.memory.len()
    }

    /// Mutable view of the physical memory, for DMA-style host models.
    pub fn physical_memory_mut(&mut self) -> &mut [Word] {
        &mut self.memory
    }

    /// Attaches a device to one of the eight architectural DEV slots.
    pub fn attach_device(&mut self, device: u8, handler: Box<dyn Device>) {
        assert!(device < 8, "CpuV3 device index {device} exceeds 3 bits");
        self.devices[usize::from(device)] = Some(handler);
    }

    /// Accesses an attached device model, e.g. for test assertions.
    pub fn device<T: 'static>(&self, device: u8) -> Option<&T> {
        self.devices
            .get(usize::from(device))?
            .as_ref()?
            .as_any()
            .downcast_ref()
    }

    /// Installs the BSRAM boot-window image (see the `boot_window` field).
    pub fn set_boot_window(&mut self, words: &[Word]) {
        self.boot_window = Some(words.to_vec().into_boxed_slice());
    }

    pub fn pc(&self) -> Word {
        self.pc
    }

    pub fn code_segment(&self) -> Word {
        self.code_segment
    }

    pub fn data_segment(&self) -> Word {
        self.data_segment
    }

    pub fn retired_words(&self) -> u64 {
        self.retired_words
    }

    pub fn pending_test(&self) -> Option<std::cmp::Ordering> {
        self.pending_test
    }

    /// Set the program counter directly. The debugger uses this to enter a
    /// linked program at its code base instead of running a register bootstrap.
    pub fn set_pc(&mut self, pc: Word) {
        self.pc = pc;
    }

    pub fn run(&mut self, maximum_steps: usize) -> Result<RunOutcome, Fault> {
        for steps in 1..=maximum_steps {
            if let StepOutcome::Halted { signal } = self.step()? {
                return Ok(RunOutcome::Halted { steps, signal });
            }
        }
        Ok(RunOutcome::StepLimit {
            steps: maximum_steps,
        })
    }

    pub fn step(&mut self) -> Result<StepOutcome, Fault> {
        if self.halted {
            return Ok(StepOutcome::Halted {
                signal: self.halt_signal,
            });
        }

        let address = self.pc;
        let fetch_address = PhysicalWordAddress::from_segment_offset(self.code_segment, address);
        let instruction = match &self.boot_window {
            Some(window) if (fetch_address.get() as usize) < window.len() => {
                Ok(window[fetch_address.get() as usize])
            }
            _ => self.read_physical(fetch_address),
        }
        .map_err(|kind| Fault {
            kind,
            address,
            instruction: 0,
        })?;
        self.pc = self.pc.wrapping_add(1);
        let opcode = instruction >> 12;

        if opcode == 0xf {
            if self.prefix.take().is_some() {
                self.retired_words += 1;
            }
            self.prefix = Some(Prefix {
                address,
                payload: instruction & 0xfff,
            });
            return Ok(StepOutcome::Running);
        }

        // FPU v2 (major 0xC / 0xD / 0xE) is a 32-bit instruction fetched as two
        // 16-bit words. The unit takes word0 and then word1; the architecture
        // retires the pair as two words. A pending PFX12 expires unused first,
        // exactly as it does before any other non-consuming instruction.
        if let Some(fpu_opcode) = FpuOpcode::from_word0(instruction) {
            if self.prefix.take().is_some() {
                self.retired_words += 1;
            }
            self.pending_test = None;
            let word1_address = self.pc;
            let fetch1 = PhysicalWordAddress::from_segment_offset(self.code_segment, word1_address);
            let word1 = match &self.boot_window {
                Some(window) if (fetch1.get() as usize) < window.len() => {
                    window[fetch1.get() as usize]
                }
                _ => self.read_physical(fetch1).map_err(|kind| Fault {
                    kind,
                    address: word1_address,
                    instruction: 0,
                })?,
            };
            self.pc = self.pc.wrapping_add(1);
            return match self.execute_fpu_pair(instruction, word1, fpu_opcode) {
                Ok(outcome) => {
                    self.retired_words += 2;
                    Ok(outcome)
                }
                Err(kind) => {
                    // Neither half retires; rewind to the first word.
                    self.pc = address;
                    Err(Fault {
                        kind,
                        address,
                        instruction,
                    })
                }
            };
        }

        let consumes_prefix = is_prefix_consumer(instruction);
        let prefix = self.prefix.take();
        // Every retired non-prefix instruction expires the pending test;
        // CMP-class instructions set it again below and conditional
        // branches consume the taken value.
        let pending = self.pending_test.take();
        let retire_words = if prefix.is_some() && consumes_prefix {
            2
        } else {
            if prefix.is_some() {
                self.retired_words += 1;
            }
            1
        };
        let fault_address = if consumes_prefix {
            prefix.map_or(address, |prefix| prefix.address)
        } else {
            address
        };

        let result = match opcode {
            0x0 | 0x1 | 0x3..=0x5 => self.execute_alu(opcode, instruction),
            0x2 => self.execute_shift_multiply(instruction, prefix),
            0x6 => self.execute_extended(instruction),
            0x7 => self.execute_device(instruction),
            0x8 => self.execute_load(instruction, prefix),
            0x9 => self.execute_store(instruction, prefix),
            0xa => self.execute_immediate(instruction, prefix),
            0xb => self.execute_branch(instruction, prefix, pending),
            // Majors C/D/E are handled as two-word FPU pairs above; no other
            // major exists. Reserved function slots inside the dispatched
            // families are rejected by the individual executors.
            _ => Err(FaultKind::InvalidInstruction),
        };
        match result {
            Ok(outcome) => {
                self.retired_words += retire_words;
                Ok(outcome)
            }
            // The faulting word does not retire, so the increment above is
            // undone by rewinding the program counter to the fault address.
            Err(kind) => {
                self.pc = fault_address;
                Err(Fault {
                    kind,
                    address: fault_address,
                    instruction,
                })
            }
        }
    }

    fn execute_alu(&mut self, opcode: Word, instruction: Word) -> ExecuteResult {
        let dst = field(instruction, 8);
        let lhs = self.registers[usize::from(field(instruction, 4))];
        let rhs = self.registers[usize::from(field(instruction, 0))];
        self.registers[usize::from(dst)] = match opcode {
            0 => lhs.wrapping_add(rhs),
            1 => lhs.wrapping_sub(rhs),
            3 => lhs & rhs,
            4 => lhs | rhs,
            5 => lhs ^ rhs,
            _ => unreachable!(),
        };
        Ok(StepOutcome::Running)
    }

    fn execute_shift_multiply(
        &mut self,
        instruction: Word,
        prefix: Option<Prefix>,
    ) -> ExecuteResult {
        let function = field(instruction, 8);
        let dst = usize::from(field(instruction, 4));
        let operand = field(instruction, 0);
        let old = self.registers[dst];
        // Every operation in the family reads `rd` before writing it, so the
        // destructive `rd == rs` case needs no special handling.
        let result = match function {
            // Register-count shift: the count is the operand register's low
            // nibble.
            0..=2 => {
                let amount = u32::from(self.registers[usize::from(operand)] & 15);
                match function {
                    0 => old.wrapping_shl(amount),
                    1 => old.wrapping_shr(amount),
                    _ => ((old as i16) >> amount) as Word,
                }
            }
            // Immediate shift: the whole operand nibble is the count.
            4..=6 => {
                let amount = u32::from(operand);
                match function {
                    4 => old.wrapping_shl(amount),
                    5 => old.wrapping_shr(amount),
                    _ => ((old as i16) >> amount) as Word,
                }
            }
            // MUL0/MUL8/MUL16: unsigned 32-bit product, keep the 16-bit window
            // after shifting right by 0, 8, or 16.
            8..=10 => {
                let product = u32::from(old) * u32::from(self.registers[usize::from(operand)]);
                (product >> (8 * (function - 8))) as Word
            }
            // MULI: unsigned immediate, shift-0 window only.
            12 => {
                let immediate = u32::from(immediate4(instruction, prefix, false));
                (u32::from(old) * immediate) as Word
            }
            _ => return Err(FaultKind::InvalidInstruction),
        };
        self.registers[dst] = result;
        Ok(StepOutcome::Running)
    }

    fn execute_load(&mut self, instruction: Word, prefix: Option<Prefix>) -> ExecuteResult {
        let dst = field(instruction, 8);
        let base = self.registers[usize::from(field(instruction, 4))];
        let address = base.wrapping_add(immediate4(instruction, prefix, true));
        let physical = self.data_address(address);
        self.registers[usize::from(dst)] = self.read_data(physical)?;
        Ok(StepOutcome::Running)
    }

    fn execute_store(&mut self, instruction: Word, prefix: Option<Prefix>) -> ExecuteResult {
        let src = field(instruction, 8);
        let base = self.registers[usize::from(field(instruction, 4))];
        let address = base.wrapping_add(immediate4(instruction, prefix, true));
        let physical = self.data_address(address);
        self.write_data(physical, self.registers[usize::from(src)])?;
        Ok(StepOutcome::Running)
    }

    fn execute_immediate(&mut self, instruction: Word, prefix: Option<Prefix>) -> ExecuteResult {
        let function = field(instruction, 8);
        let dst = field(instruction, 4);
        let old = self.registers[usize::from(dst)];
        // The effective value is the prefixed 16-bit pattern when a prefix is
        // present and the bare nibble otherwise. The signed and unsigned
        // readings are the two interpretations of that one pattern, so they are
        // derived from it rather than decoded twice.
        let nibble = instruction & 15;
        let wide = prefix.map(|prefix| (prefix.payload << 4) | nibble);
        let signed = wide.unwrap_or_else(|| sign_extend(nibble, 4));
        let unsigned = wide.unwrap_or(nibble);
        match function {
            // CMPSI/CMPUI set the pending test result and write no register.
            12 => {
                self.pending_test = Some((old as i16).cmp(&(signed as i16)));
                return Ok(StepOutcome::Running);
            }
            13 => {
                self.pending_test = Some(old.cmp(&unsigned));
                return Ok(StepOutcome::Running);
            }
            _ => {}
        }
        let result = match function {
            // ADDI/SUBI read the unprefixed immediate as an unsigned u4; the
            // prefixed form adds/subtracts the full 16-bit pattern.
            0 => old.wrapping_add(unsigned),
            1 => old.wrapping_sub(unsigned),
            2 if wide.is_some() => unsigned,
            2 => signed,
            3 => unsigned,
            4 => old & unsigned,
            5 => old | unsigned,
            6 => old ^ unsigned,
            // LDC/ADDC index the shared constant table; a pending prefix
            // expires unused (these never consume it).
            7 => CONSTANT_TABLE[usize::from(nibble)],
            8 => Word::from(old == signed),
            9 => Word::from((old as i16) < (signed as i16)),
            10 => Word::from(old < unsigned),
            11 => old.wrapping_add(CONSTANT_TABLE[usize::from(nibble)]),
            _ => return Err(FaultKind::InvalidInstruction),
        };
        self.registers[usize::from(dst)] = result;
        Ok(StepOutcome::Running)
    }

    fn execute_branch(
        &mut self,
        instruction: Word,
        prefix: Option<Prefix>,
        pending: Option<Ordering>,
    ) -> ExecuteResult {
        let function = field(instruction, 8);
        let offset = prefix.map_or_else(
            || sign_extend(instruction & 0xff, 8),
            |prefix| ((prefix.payload & 0xff) << 8) | (instruction & 0xff),
        );
        let condition = |function: u8| match function & 7 {
            0 => pending == Some(Ordering::Equal),
            1 => pending != Some(Ordering::Equal),
            2 => pending == Some(Ordering::Less),
            3 => pending != Some(Ordering::Less),
            4 => pending == Some(Ordering::Greater),
            _ => pending != Some(Ordering::Greater),
        };
        match function {
            // Conditional branches consume the pending test result, whether
            // or not the branch is taken.
            0..=5 => {
                if pending.is_none() {
                    return Err(FaultKind::InvalidInstruction);
                }
                if condition(function) {
                    self.pc = self.pc.wrapping_add(offset);
                }
            }
            // JREL: unconditional relative jump, no link.
            6 => self.pc = self.pc.wrapping_add(offset),
            // JALREL: link the fall-through address into r14, then jump.
            7 => {
                let next = self.pc;
                self.pc = next.wrapping_add(offset);
                self.registers[usize::from(LINK_REGISTER)] = next;
            }
            // Conditional moves consume the pending test result, whether or
            // not the move writes.
            8..=13 => {
                if pending.is_none() {
                    return Err(FaultKind::InvalidInstruction);
                }
                if condition(function) {
                    self.registers[usize::from(field(instruction, 4))] =
                        self.registers[usize::from(field(instruction, 0))];
                }
            }
            // JREG is canonically `B E 0 target`.
            14 if field(instruction, 4) == 0 => {
                self.pc = self.registers[usize::from(field(instruction, 0))];
            }
            // JALR is canonically `B F E target`: link r14, then jump.
            15 if field(instruction, 4) == LINK_REGISTER => {
                let target = self.registers[usize::from(field(instruction, 0))];
                self.registers[usize::from(LINK_REGISTER)] = self.pc;
                self.pc = target;
            }
            _ => return Err(FaultKind::InvalidInstruction),
        }
        Ok(StepOutcome::Running)
    }

    fn execute_device(&mut self, instruction: Word) -> ExecuteResult {
        let device = usize::from(field(instruction, 8) & 7);
        let channel = field(instruction, 4);
        let register = usize::from(field(instruction, 0));
        if instruction & 0x800 != 0 {
            let value = self.registers[register];
            if let Some(handler) = self.devices[device].as_mut() {
                handler.write(&mut self.memory, channel, value);
            }
        } else {
            self.registers[register] = self.devices[device]
                .as_mut()
                .map_or(0, |handler| handler.read(&mut self.memory, channel));
        }
        Ok(StepOutcome::Running)
    }

    fn execute_fpu_pair(&mut self, word0: Word, word1: Word, opcode: FpuOpcode) -> ExecuteResult {
        match opcode {
            FpuOpcode::Vector => self.execute_fpu_vector(word0, word1),
            FpuOpcode::Scalar => self.execute_fpu_scalar(word0, word1),
            FpuOpcode::Aux => self.execute_fpu_aux(word0, word1),
        }
    }

    fn fpu_read(&self, index: usize) -> i32 {
        self.fpu_registers[index]
    }

    fn fpu_write(&mut self, index: usize, value: i32) {
        self.fpu_registers[index] = value;
    }

    /// `0xC` VECTOR: a consecutive register-range view. Lane `i` computes
    /// `A = Fa + i`, `B = Fb + i` (or `Fb` for `VMULS`), `D = Fd + i`. The
    /// destination/source partial-overlap rule (design section 3.2) is a
    /// software contract; the architecture executes lanes in increasing order.
    fn execute_fpu_vector(&mut self, word0: Word, word1: Word) -> ExecuteResult {
        let fa = usize::from(fpu_fa(word0));
        let fb = usize::from(fpu_fb(word0));
        let fd = usize::from(fpu_fd(word1));
        let mode = fpu_vector_mode(word1);
        let len_field = fpu_vector_len_field(word1);
        let subop_field = fpu_vector_subop_field(word1);
        let Some(len) = FpuVectorLength::from_field(len_field) else {
            return Err(FaultKind::InvalidInstruction);
        };
        let Some(subop) = FpuVectorSubop::from_field(subop_field) else {
            return Err(FaultKind::InvalidInstruction);
        };
        // The same contract the strict builder asserts and the decoder reports;
        // reserved modes and ranges past F63 fault here identically.
        if fpu_vector_field_error(
            fpu_fa(word0),
            fpu_fb(word0),
            fpu_fd(word1),
            len,
            subop,
            mode,
        )
        .is_some()
        {
            return Err(FaultKind::InvalidInstruction);
        }
        let lanes = usize::from(len.lanes());

        // Unary subops ignore B; VMULS broadcasts lane 0; DOT uses a stride.
        let unary = matches!(
            subop,
            FpuVectorSubop::VAbs
                | FpuVectorSubop::VNeg
                | FpuVectorSubop::VFloor
                | FpuVectorSubop::VCeil
                | FpuVectorSubop::VRound
                | FpuVectorSubop::VTrunc
                | FpuVectorSubop::VMove
        );
        let dot = matches!(
            subop,
            FpuVectorSubop::Dot | FpuVectorSubop::DotAdd | FpuVectorSubop::DotStore
        );
        if dot {
            let stride = FpuDotStride::from_mode(mode).expect("validated DOT stride");
            let mut acc = if subop == FpuVectorSubop::DotAdd {
                self.fpu_accumulator
            } else {
                0
            };
            for lane in 0..lanes {
                let a = self.fpu_read(fa + lane);
                let b = self.fpu_read(fb + lane * usize::from(stride.step()));
                acc = fix32_accumulate_product(acc, a, b);
            }
            match subop {
                FpuVectorSubop::Dot | FpuVectorSubop::DotAdd => self.fpu_accumulator = acc,
                _ => {
                    // DOTSTORE narrows once and leaves ACC clean.
                    self.fpu_write(fd, fix32_from_acc(acc));
                    self.fpu_accumulator = 0;
                }
            }
            return Ok(StepOutcome::Running);
        }
        for lane in 0..lanes {
            let a = self.fpu_read(fa + lane);
            let b = if unary {
                0
            } else if subop == FpuVectorSubop::VMulS {
                self.fpu_read(fb)
            } else {
                self.fpu_read(fb + lane)
            };
            let result = match subop {
                FpuVectorSubop::VAdd => fix32_add(a, b),
                FpuVectorSubop::VSub => fix32_sub(a, b),
                FpuVectorSubop::VMul | FpuVectorSubop::VMulS => fix32_mul(a, b),
                FpuVectorSubop::VMin => a.min(b),
                FpuVectorSubop::VMax => a.max(b),
                FpuVectorSubop::VAbs => fix32_abs(a),
                FpuVectorSubop::VNeg => fix32_neg(a),
                FpuVectorSubop::VFloor => fix32_floor(a),
                FpuVectorSubop::VCeil => fix32_ceil(a),
                FpuVectorSubop::VRound => fix32_round(a),
                FpuVectorSubop::VTrunc => fix32_trunc(a),
                FpuVectorSubop::VMove => a,
                FpuVectorSubop::Dot | FpuVectorSubop::DotAdd | FpuVectorSubop::DotStore => {
                    unreachable!()
                }
            };
            self.fpu_write(fd + lane, result);
        }
        Ok(StepOutcome::Running)
    }

    /// `0xD` SCALAR: `Fa`/`Fb` are scalar sources, `Fd` the scalar destination.
    fn execute_fpu_scalar(&mut self, word0: Word, word1: Word) -> ExecuteResult {
        let fa = usize::from(fpu_fa(word0));
        let fb = usize::from(fpu_fb(word0));
        let fd = usize::from(fpu_fd(word1));
        let mode = fpu_mode(word1);
        let subop_field = fpu_scalar_subop_field(word1);
        let Some(subop) = FpuScalarSubop::from_field(subop_field) else {
            return Err(FaultKind::InvalidInstruction);
        };
        if fpu_scalar_field_error(fpu_fa(word0), fpu_fb(word0), fpu_fd(word1), subop, mode)
            .is_some()
        {
            return Err(FaultKind::InvalidInstruction);
        }
        let a = self.fpu_read(fa);
        let b = self.fpu_read(fb);
        match subop {
            FpuScalarSubop::Add => self.fpu_write(fd, fix32_add(a, b)),
            FpuScalarSubop::Sub => self.fpu_write(fd, fix32_sub(a, b)),
            FpuScalarSubop::Mul => self.fpu_write(fd, fix32_mul(a, b)),
            FpuScalarSubop::Min => self.fpu_write(fd, a.min(b)),
            FpuScalarSubop::Max => self.fpu_write(fd, a.max(b)),
            FpuScalarSubop::Abs => self.fpu_write(fd, fix32_abs(a)),
            FpuScalarSubop::Neg => self.fpu_write(fd, fix32_neg(a)),
            FpuScalarSubop::Floor => self.fpu_write(fd, fix32_floor(a)),
            FpuScalarSubop::Ceil => self.fpu_write(fd, fix32_ceil(a)),
            FpuScalarSubop::Round => self.fpu_write(fd, fix32_round(a)),
            FpuScalarSubop::Trunc => self.fpu_write(fd, fix32_trunc(a)),
            FpuScalarSubop::Cmp => self.pending_test = Some(fix32_compare(a, b)),
            FpuScalarSubop::Rcp => self.fpu_write(fd, rcp_q16(a)),
            FpuScalarSubop::Rsqrt => self.fpu_write(fd, rsqrt_q16(a)),
            FpuScalarSubop::SinCos => {
                let sc_mode = FpuSinCosMode::from_mode(mode).expect("validated SINCOS mode");
                let (sin, cos) = sincos_q16(a);
                match sc_mode {
                    FpuSinCosMode::SinCos => {
                        self.fpu_write(fd, sin);
                        self.fpu_write(fd + 1, cos);
                    }
                    FpuSinCosMode::Sin => self.fpu_write(fd, sin),
                    FpuSinCosMode::Cos => self.fpu_write(fd, cos),
                }
            }
            FpuScalarSubop::Mov => self.fpu_write(fd, a),
        }
        Ok(StepOutcome::Running)
    }

    /// `0xE` AUX: integer-register bridges and FLD/FST/FLDV/FSTV.
    fn execute_fpu_aux(&mut self, word0: Word, word1: Word) -> ExecuteResult {
        let kind = FpuAuxKind::from_field((word0 & 0b11) as u8);
        let x = usize::from(fpu_aux_x(word0));
        let fa = usize::from(fpu_aux_fa(word0));
        let fd = usize::from(fpu_fd(word1));
        let mode = fpu_mode(word1);
        let subop_field = fpu_scalar_subop_field(word1);
        let Some(subop) = FpuAuxSubop::from_field(subop_field) else {
            return Err(FaultKind::InvalidInstruction);
        };
        // The shared contract rejects unimplemented kinds, reserved FLD/FST
        // mode bits, and register-range overflow for builder, decoder and sim.
        if fpu_aux_field_error(kind, fpu_aux_fa(word0), fpu_fd(word1), subop, mode).is_some() {
            return Err(FaultKind::InvalidInstruction);
        }
        let gpr = self.registers[x];
        match subop {
            FpuAuxSubop::Fld => {
                let lanes = usize::from(mode) + 1;
                for lane in 0..lanes {
                    // Low half first, as FST and the core memory beats.
                    let low =
                        self.read_data(self.data_address(gpr.wrapping_add(2 * lane as Word)))?;
                    let high =
                        self.read_data(self.data_address(gpr.wrapping_add(2 * lane as Word + 1)))?;
                    self.fpu_write(fd + lane, ((u32::from(high) << 16) | u32::from(low)) as i32);
                }
            }
            FpuAuxSubop::Fst => {
                let lanes = usize::from(mode) + 1;
                for lane in 0..lanes {
                    let value = self.fpu_read(fa + lane) as u32;
                    self.write_data(
                        self.data_address(gpr.wrapping_add(2 * lane as Word)),
                        (value & 0xffff) as Word,
                    )?;
                    self.write_data(
                        self.data_address(gpr.wrapping_add(2 * lane as Word + 1)),
                        (value >> 16) as Word,
                    )?;
                }
            }
            FpuAuxSubop::Ilo2f => {
                let current = self.fpu_read(fd);
                self.fpu_write(fd, (current & !0xffff) | i32::from(gpr));
            }
            FpuAuxSubop::Ihi2f => {
                let current = self.fpu_read(fd);
                self.fpu_write(fd, (current & 0xffff) | (i32::from(gpr) << 16));
            }
            FpuAuxSubop::Flo2i => self.registers[x] = (self.fpu_read(fa) & 0xffff) as Word,
            FpuAuxSubop::Fhi2i => self.registers[x] = ((self.fpu_read(fa) >> 16) & 0xffff) as Word,
            FpuAuxSubop::I16tof => self.fpu_write(fd, fix32_from_i16(gpr as i16)),
            FpuAuxSubop::Ftoi16 => self.registers[x] = fix32_to_i16(self.fpu_read(fa)) as Word,
        }
        Ok(StepOutcome::Running)
    }

    fn execute_extended(&mut self, instruction: Word) -> ExecuteResult {
        let function = field(instruction, 8);
        let dst = field(instruction, 4);
        let src = field(instruction, 0);
        match function {
            0 => self.registers[usize::from(dst)] = self.registers[usize::from(src)],
            1 => self.registers[usize::from(dst)] = !self.registers[usize::from(src)],
            2 => self.registers[usize::from(dst)] = self.registers[usize::from(src)].wrapping_neg(),
            3 => {
                self.registers[usize::from(dst)] =
                    sign_extend(self.registers[usize::from(src)] & 0xff, 8)
            }
            4 => {
                self.registers[usize::from(dst)] =
                    self.registers[usize::from(src)].leading_zeros() as Word
            }
            5 => {
                self.registers[usize::from(dst)] =
                    self.registers[usize::from(src)].count_ones() as Word
            }
            6 => {
                self.registers[usize::from(dst)] =
                    Word::from(self.registers[usize::from(dst)] == self.registers[usize::from(src)])
            }
            8 => {
                self.registers[usize::from(dst)] = Word::from(
                    (self.registers[usize::from(dst)] as i16)
                        < (self.registers[usize::from(src)] as i16),
                )
            }
            9 => {
                self.registers[usize::from(dst)] =
                    Word::from(self.registers[usize::from(dst)] < self.registers[usize::from(src)])
            }
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
            12 => {
                // SIGNAL rs, type4. Type 0 halts and latches the value at the
                // retirement edge; nonzero types retire as a NOP in hardware
                // and surface here as a model-side event.
                let signal_type = src;
                let value = self.registers[usize::from(dst)];
                if signal_type == 0 {
                    self.halt_signal = value;
                    self.halted = true;
                    return Ok(StepOutcome::Halted {
                        signal: self.halt_signal,
                    });
                }
                // Retirement accounting (including a consumed prefix) happens
                // in step() like for every other instruction.
                return Ok(StepOutcome::Signaled(SignalEvent { signal_type, value }));
            }
            13 => {
                self.registers[usize::from(dst)] = match src {
                    value if value == SpecialRegister::CodeSegment as u8 => self.code_segment,
                    value if value == SpecialRegister::DataSegment as u8 => self.data_segment,
                    _ => return Err(FaultKind::InvalidInstruction),
                }
            }
            14 if dst == SpecialRegister::DataSegment as u8 => {
                self.data_segment = self.registers[usize::from(src)];
            }
            15 => {
                self.code_segment = self.registers[usize::from(dst)];
                self.pc = self.registers[usize::from(src)];
            }
            _ => return Err(FaultKind::InvalidInstruction),
        }
        Ok(StepOutcome::Running)
    }

    fn data_address(&self, offset: Word) -> PhysicalWordAddress {
        PhysicalWordAddress::from_segment_offset(self.data_segment, offset)
    }

    fn read_data(&mut self, address: PhysicalWordAddress) -> Result<Word, FaultKind> {
        self.read_physical(address)
    }

    fn write_data(&mut self, address: PhysicalWordAddress, value: Word) -> Result<(), FaultKind> {
        self.write_physical(address, value)
    }

    fn read_physical(&self, address: PhysicalWordAddress) -> Result<Word, FaultKind> {
        self.memory
            .get(address.get() as usize)
            .copied()
            .ok_or(FaultKind::PhysicalAddressOutOfRange { address })
    }

    fn write_physical(
        &mut self,
        address: PhysicalWordAddress,
        value: Word,
    ) -> Result<(), FaultKind> {
        let word = self
            .memory
            .get_mut(address.get() as usize)
            .ok_or(FaultKind::PhysicalAddressOutOfRange { address })?;
        *word = value;
        Ok(())
    }
}

type ExecuteResult = Result<StepOutcome, FaultKind>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProgramLoadError {
    pub base: PhysicalWordAddress,
    pub words: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RunOutcome {
    Halted { steps: usize, signal: Word },
    StepLimit { steps: usize },
}

fn field(instruction: Word, shift: u32) -> u8 {
    ((instruction >> shift) & 15) as u8
}

fn immediate4(instruction: Word, prefix: Option<Prefix>, signed: bool) -> Word {
    prefix.map_or_else(
        || {
            if signed {
                sign_extend(instruction & 15, 4)
            } else {
                instruction & 15
            }
        },
        |prefix| (prefix.payload << 4) | (instruction & 15),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        alu, branch, compare_signed, compare_unsigned, conditional_move, device_receive,
        device_send, fpu_aux, fpu_aux_raw, fpu_scalar, fpu_vector, fpu_vector_raw, halt,
        immediate_signed, immediate_unsigned, jump_and_link_register, jump_and_link_relative,
        jump_register, jump_relative, jump_segment, load, load_immediate16, move_register,
        multiply, multiply_immediate, nop, population_count, prefix12, prefixed, prefixed_branch,
        read_special, set_less_than_signed, set_less_than_unsigned, shift_immediate,
        shift_register, signal, store, write_data_segment, AluOp, FpuAuxKind, FpuAuxSubop,
        FpuDotStride, FpuScalarSubop, FpuVectorLength, FpuVectorSubop, ImmediateOp, MultiplyWindow,
        ShiftOp, SpecialRegister, TestCondition,
    };

    #[test]
    fn executes_loop_and_unified_memory_round_trip() {
        let mut program = vec![];
        program.extend(load_immediate16(0, 0));
        program.extend(load_immediate16(1, 5));
        program.extend(load_immediate16(2, 0x4000));
        program.extend([
            alu(AluOp::Add, 0, 0, 1),
            immediate_unsigned(ImmediateOp::Sub, 1, 1),
            immediate_signed(ImmediateOp::CompareSigned, 1, 0),
            branch(TestCondition::NotEqual, -4),
            store(0, 2, 0),
            load(3, 2, 0),
            halt(),
        ]);
        let mut machine = CpuV3Sim::default();
        machine.load_program(0, &program).unwrap();

        assert_eq!(
            machine.run(100).unwrap(),
            RunOutcome::Halted {
                steps: 29,
                signal: 15
            }
        );
        assert_eq!(machine.register(3), Some(15));
        assert_eq!(machine.memory(0x4000), 15);
        assert_eq!(machine.retired_words(), 29);
    }

    #[test]
    fn prefix_and_consumer_retire_as_one_wide_operation() {
        let mut machine = CpuV3Sim::default();
        machine.load_program(0, &[0xf400, 0x8100, halt()]).unwrap();
        machine.load_program(0x4000, &[0xbeef]).unwrap();
        assert_eq!(machine.step(), Ok(StepOutcome::Running));
        assert_eq!(machine.retired_words(), 0);
        assert_eq!(machine.step(), Ok(StepOutcome::Running));
        assert_eq!(machine.register(1), Some(0xbeef));
        assert_eq!(machine.retired_words(), 2);
    }

    #[test]
    fn non_consumer_retires_and_expires_the_prefix() {
        let mut machine = CpuV3Sim::default();
        machine
            .load_program(
                0,
                &[
                    0xfabc,
                    move_register(1, 1),
                    immediate_unsigned(ImmediateOp::LoadUnsigned, 3, 0xd),
                    halt(),
                ],
            )
            .unwrap();
        assert_eq!(
            machine.run(10).unwrap(),
            RunOutcome::Halted {
                steps: 4,
                signal: 0
            }
        );
        assert_eq!(machine.register(3), Some(13));
        assert_eq!(machine.retired_words(), 4);
    }

    #[test]
    fn constant_table_ops_cover_signed_entries_and_expire_the_prefix() {
        let mut machine = CpuV3Sim::default();
        machine
            .load_program(
                0,
                &[
                    crate::load_constant(1, 0),  // r1 = 8
                    crate::load_constant(2, 7),  // r2 = 512
                    crate::load_constant(3, 9),  // r3 = -16 (0xfff0)
                    crate::load_constant(4, 15), // r4 = -512 (0xfe00)
                    crate::add_constant(1, 4),   // r1 = 8 + 64 = 72
                    crate::add_constant(3, 15),  // r3 = -16 + -512 = -528 (0xfdf0)
                    // A pending prefix expires unused before the non-consuming
                    // ADDC and retires separately.
                    prefix12(0xabc),
                    crate::add_constant(1, 12), // r1 = 72 + -64 = 8
                    halt(),
                ],
            )
            .unwrap();
        assert_eq!(
            machine.run(20).unwrap(),
            RunOutcome::Halted {
                steps: 9,
                signal: 0
            }
        );
        assert_eq!(machine.register(1), Some(8));
        assert_eq!(machine.register(2), Some(512));
        assert_eq!(machine.register(3), Some(0xfdf0));
        assert_eq!(machine.register(4), Some(0xfe00));
        assert_eq!(machine.retired_words(), 9);
    }

    #[test]
    fn addi_subi_read_the_unprefixed_immediate_as_unsigned() {
        let mut machine = CpuV3Sim::default();
        machine
            .load_program(
                0,
                &[
                    immediate_unsigned(ImmediateOp::Add, 1, 15), // r1 = 15
                    immediate_unsigned(ImmediateOp::Add, 1, 0),  // r1 = 15
                    immediate_unsigned(ImmediateOp::Sub, 1, 15), // r1 = 0
                    immediate_unsigned(ImmediateOp::Sub, 1, 0),  // r1 = 0
                    // The high nibbles are additions, not negative offsets:
                    // the old signed reading would have made this r1 = -1.
                    immediate_unsigned(ImmediateOp::Add, 1, 8), // r1 = 8
                    halt(),
                ],
            )
            .unwrap();
        assert_eq!(
            machine.run(10).unwrap(),
            RunOutcome::Halted {
                steps: 6,
                signal: 0
            }
        );
        assert_eq!(machine.register(1), Some(8));
    }

    #[test]
    fn prefixed_addi_subi_use_the_full_16_bit_pattern() {
        let mut machine = CpuV3Sim::default();
        let [p0, add] = prefixed(immediate_unsigned(ImmediateOp::Add, 1, 0), 0x0010);
        let [p1, sub] = prefixed(immediate_unsigned(ImmediateOp::Sub, 1, 0), 0x0008);
        machine
            .load_program(0, &[p0, add, p1, sub, halt()])
            .unwrap();
        assert_eq!(
            machine.run(10).unwrap(),
            RunOutcome::Halted {
                steps: 5,
                signal: 0
            }
        );
        assert_eq!(machine.register(1), Some(8));
    }

    #[test]
    fn fpu_scalar_two_word_instructions_retire_as_pairs() {
        let mut program = vec![];
        program.extend(load_immediate16(1, 3));
        program.extend(load_immediate16(2, 4));
        program.extend(fpu_aux(
            FpuAuxKind::IntegerRegister,
            1,
            0,
            0,
            FpuAuxSubop::I16tof,
            0,
        )); // f0 = 3.0
        program.extend(fpu_aux(
            FpuAuxKind::IntegerRegister,
            2,
            0,
            1,
            FpuAuxSubop::I16tof,
            0,
        )); // f1 = 4.0
        program.extend(fpu_scalar(0, 1, 2, FpuScalarSubop::Add, 0)); // f2 = 7.0
        program.extend(fpu_scalar(0, 1, 3, FpuScalarSubop::Mul, 0)); // f3 = 12.0
        program.extend(fpu_scalar(2, 0, 4, FpuScalarSubop::Mov, 0)); // f4 = 7.0
        program.extend(fpu_aux(
            FpuAuxKind::IntegerRegister,
            3,
            3,
            0,
            FpuAuxSubop::Ftoi16,
            0,
        )); // r3 = trunc(f3) = 12
        program.push(halt());
        let words = program.len();
        let mut machine = CpuV3Sim::default();
        machine.load_program(0, &program).unwrap();
        assert!(matches!(
            machine.run(64).unwrap(),
            RunOutcome::Halted { .. }
        ));
        assert_eq!(machine.fpu_register(0), Some(3 << 16));
        assert_eq!(machine.fpu_register(1), Some(4 << 16));
        assert_eq!(machine.fpu_register(2), Some(7 << 16));
        assert_eq!(machine.fpu_register(3), Some(12 << 16));
        assert_eq!(machine.fpu_register(4), Some(7 << 16));
        assert_eq!(machine.register(3), Some(12));
        // Every FPU instruction retires two words.
        assert_eq!(machine.retired_words(), words as u64);
    }

    #[test]
    fn fpu_vector_lanes_dot_and_stride_follow_the_range_model() {
        let mut program = vec![];
        program.extend(load_immediate16(1, 0x0100));
        program.extend(fpu_aux(
            FpuAuxKind::IntegerRegister,
            1,
            0,
            0,
            FpuAuxSubop::Fld,
            3,
        )); // FLDV4 f0..f3 = 1,2,3,4
        program.extend(fpu_vector(
            0,
            0,
            4,
            FpuVectorLength::Vec4,
            FpuVectorSubop::VAdd,
            0,
        )); // f4..f7 = 2,4,6,8
        program.extend(fpu_vector(
            0,
            0,
            8,
            FpuVectorLength::Vec4,
            FpuVectorSubop::VMul,
            0,
        )); // f8..f11 = 1,4,9,16
        program.extend(fpu_vector(
            0,
            0,
            16,
            FpuVectorLength::Vec4,
            FpuVectorSubop::VMulS,
            0,
        )); // f16..f19 = 1,2,3,4
        program.extend(fpu_vector(
            0,
            0,
            12,
            FpuVectorLength::Vec4,
            FpuVectorSubop::DotStore,
            0,
        )); // f12 = 30.0
        program.push(halt());
        let mut machine = CpuV3Sim::default();
        machine.load_program(0, &program).unwrap();
        machine
            .load_program(
                0x0100,
                &[
                    0x0000, 0x0001, 0x0000, 0x0002, 0x0000, 0x0003, 0x0000, 0x0004,
                ],
            )
            .unwrap();
        assert!(matches!(
            machine.run(64).unwrap(),
            RunOutcome::Halted { .. }
        ));
        assert_eq!(
            [
                machine.fpu_register(0),
                machine.fpu_register(1),
                machine.fpu_register(2),
                machine.fpu_register(3),
            ],
            [Some(1 << 16), Some(2 << 16), Some(3 << 16), Some(4 << 16)]
        );
        assert_eq!(
            [
                machine.fpu_register(4),
                machine.fpu_register(5),
                machine.fpu_register(6),
                machine.fpu_register(7),
            ],
            [Some(2 << 16), Some(4 << 16), Some(6 << 16), Some(8 << 16)]
        );
        assert_eq!(
            [
                machine.fpu_register(8),
                machine.fpu_register(9),
                machine.fpu_register(10),
                machine.fpu_register(11),
            ],
            [Some(1 << 16), Some(4 << 16), Some(9 << 16), Some(16 << 16)]
        );
        assert_eq!(
            [
                machine.fpu_register(16),
                machine.fpu_register(17),
                machine.fpu_register(18),
                machine.fpu_register(19),
            ],
            [Some(1 << 16), Some(2 << 16), Some(3 << 16), Some(4 << 16)]
        );
        assert_eq!(machine.fpu_register(12), Some(30 << 16));
        assert_eq!(machine.fpu_accumulator(), 0);
    }

    #[test]
    fn fpu_dot_stride_reads_the_second_source_with_a_step() {
        let mut program = vec![];
        program.extend(load_immediate16(1, 0x0100));
        program.extend(fpu_aux(
            FpuAuxKind::IntegerRegister,
            1,
            0,
            0,
            FpuAuxSubop::Fld,
            3,
        )); // FLDV4 f0..f3 = 1,2,3,4
            // B = f8, f11, f14, f17 (stride 3), all set to 1.0 by I16TOF.
        program.extend(load_immediate16(2, 1));
        for fd in [8u8, 11, 14, 17] {
            program.extend(fpu_aux(
                FpuAuxKind::IntegerRegister,
                2,
                0,
                fd,
                FpuAuxSubop::I16tof,
                0,
            ));
        }
        program.extend(fpu_vector(
            0,
            8,
            20,
            FpuVectorLength::Vec4,
            FpuVectorSubop::DotStore,
            FpuDotStride::Stride3 as u8,
        )); // f20 = 1+2+3+4 = 10.0
        program.push(halt());
        let mut machine = CpuV3Sim::default();
        machine.load_program(0, &program).unwrap();
        machine
            .load_program(
                0x0100,
                &[
                    0x0000, 0x0001, 0x0000, 0x0002, 0x0000, 0x0003, 0x0000, 0x0004,
                ],
            )
            .unwrap();
        assert!(matches!(
            machine.run(64).unwrap(),
            RunOutcome::Halted { .. }
        ));
        assert_eq!(machine.fpu_register(20), Some(10 << 16));
    }

    #[test]
    fn fpu_aux_memory_moves_low_half_first_and_bridges_raw_halves() {
        let mut program = vec![];
        program.extend(load_immediate16(1, 0x0100));
        program.extend(load_immediate16(2, 0x0102));
        program.extend(load_immediate16(3, 0xbeef));
        program.extend(fpu_aux(
            FpuAuxKind::IntegerRegister,
            1,
            0,
            0,
            FpuAuxSubop::Fld,
            0,
        )); // f0 = {mem[0x0101], mem[0x0100]}
        program.extend(fpu_aux(
            FpuAuxKind::IntegerRegister,
            2,
            0,
            0,
            FpuAuxSubop::Fst,
            0,
        )); // mem[0x0102..] = f0, low half first
        program.extend(fpu_aux(
            FpuAuxKind::IntegerRegister,
            3,
            0,
            1,
            FpuAuxSubop::Ilo2f,
            0,
        )); // f1 low = 0xbeef
        program.extend(fpu_aux(
            FpuAuxKind::IntegerRegister,
            3,
            0,
            2,
            FpuAuxSubop::Ihi2f,
            0,
        )); // f2 high = 0xbeef
        program.extend(fpu_aux(
            FpuAuxKind::IntegerRegister,
            4,
            0,
            0,
            FpuAuxSubop::Flo2i,
            0,
        )); // r4 = f0 low
        program.extend(fpu_aux(
            FpuAuxKind::IntegerRegister,
            5,
            0,
            0,
            FpuAuxSubop::Fhi2i,
            0,
        )); // r5 = f0 high
        program.push(halt());
        let mut machine = CpuV3Sim::default();
        machine.load_program(0, &program).unwrap();
        machine.load_program(0x0100, &[0x1234, 0x5678]).unwrap();
        assert!(matches!(
            machine.run(64).unwrap(),
            RunOutcome::Halted { .. }
        ));
        assert_eq!(machine.fpu_register(0), Some(0x5678_1234));
        assert_eq!(machine.memory(0x0102), 0x1234);
        assert_eq!(machine.memory(0x0103), 0x5678);
        assert_eq!(machine.fpu_register(1), Some(0x0000_beef));
        assert_eq!(machine.fpu_register(2), Some(0xbeef_0000_u32 as i32));
        assert_eq!(machine.register(4), Some(0x1234));
        assert_eq!(machine.register(5), Some(0x5678));
    }

    /// Runs `RCP`, `RSQRT` and dual `SINCOS` on one raw Q16.16 input and
    /// returns `(rcp, rsqrt, sin, cos)`.
    fn run_special(input: i32) -> (i32, i32, i32, i32) {
        let raw = input as u32;
        let mut program = vec![];
        program.extend(load_immediate16(1, 0x0100));
        program.extend(fpu_aux(
            FpuAuxKind::IntegerRegister,
            1,
            0,
            0,
            FpuAuxSubop::Fld,
            0,
        )); // f0 = input
        program.extend(fpu_scalar(0, 0, 1, FpuScalarSubop::Rcp, 0));
        program.extend(fpu_scalar(0, 0, 2, FpuScalarSubop::Rsqrt, 0));
        program.extend(fpu_scalar(0, 0, 3, FpuScalarSubop::SinCos, 0));
        program.push(halt());
        let mut machine = CpuV3Sim::default();
        machine.load_program(0, &program).unwrap();
        machine
            .load_program(0x0100, &[raw as u16, (raw >> 16) as u16])
            .unwrap();
        assert!(matches!(
            machine.run(64).unwrap(),
            RunOutcome::Halted { .. }
        ));
        (
            machine.fpu_register(1).unwrap(),
            machine.fpu_register(2).unwrap(),
            machine.fpu_register(3).unwrap(),
            machine.fpu_register(4).unwrap(),
        )
    }

    #[test]
    fn fpu_special_functions_are_bit_exact_with_the_reference_model() {
        for input in [
            0,
            1,
            -1,
            0x0001_0000,
            -0x0001_0000,
            0x0002_0000,
            0x7fff_ffff,
            i32::MIN,
            0x1234_5678,
            -0x1234_5678,
        ] {
            let (sin, cos) = sincos_q16(input);
            assert_eq!(
                run_special(input),
                (rcp_q16(input), rsqrt_q16(input), sin, cos),
                "input {input:#010x}"
            );
        }
    }

    #[test]
    fn fpu_scalar_arithmetic_wraps_and_rounds_half_up() {
        let mut program = vec![];
        program.extend(load_immediate16(1, 0x7fff));
        program.extend(load_immediate16(2, 1));
        program.extend(fpu_aux(
            FpuAuxKind::IntegerRegister,
            1,
            0,
            0,
            FpuAuxSubop::I16tof,
            0,
        )); // f0 = 32767.0
        program.extend(fpu_aux(
            FpuAuxKind::IntegerRegister,
            2,
            0,
            1,
            FpuAuxSubop::I16tof,
            0,
        )); // f1 = 1.0
        program.extend(fpu_scalar(0, 1, 2, FpuScalarSubop::Add, 0)); // wraps to i32::MIN
        program.extend(fpu_aux(
            FpuAuxKind::IntegerRegister,
            3,
            2,
            0,
            FpuAuxSubop::Ftoi16,
            0,
        )); // r3 = -32768 (0x8000)
        program.extend(load_immediate16(4, 0x0100));
        program.extend(fpu_aux(
            FpuAuxKind::IntegerRegister,
            4,
            0,
            4,
            FpuAuxSubop::Fld,
            0,
        )); // f4 = 0.5 from memory
        program.extend(fpu_scalar(4, 0, 5, FpuScalarSubop::Round, 0)); // 0.5 -> 1.0
        program.extend(fpu_aux(
            FpuAuxKind::IntegerRegister,
            6,
            5,
            0,
            FpuAuxSubop::Ftoi16,
            0,
        )); // r6 = 1
        program.extend(load_immediate16(7, 0x0102));
        program.extend(fpu_aux(
            FpuAuxKind::IntegerRegister,
            7,
            0,
            6,
            FpuAuxSubop::Fld,
            0,
        )); // f6 = -0.5 from memory
        program.extend(fpu_scalar(6, 0, 7, FpuScalarSubop::Round, 0)); // -0.5 -> 0.0
        program.extend(fpu_aux(
            FpuAuxKind::IntegerRegister,
            8,
            7,
            0,
            FpuAuxSubop::Ftoi16,
            0,
        )); // r8 = 0
        program.push(halt());
        let mut machine = CpuV3Sim::default();
        machine.load_program(0, &program).unwrap();
        machine
            .load_program(
                0x0100,
                &[0x8000, 0x0000, 0x8000, 0xffff], // +0.5, -0.5 (low half first)
            )
            .unwrap();
        assert!(matches!(
            machine.run(128).unwrap(),
            RunOutcome::Halted { .. }
        ));
        assert_eq!(machine.register(3), Some(0x8000));
        assert_eq!(machine.register(6), Some(1));
        assert_eq!(machine.register(8), Some(0));
    }

    #[test]
    fn fpu_out_of_range_register_ranges_fault() {
        // FLDV2 with Fd = 63 needs F63 and F64; F64 does not exist.
        let mut machine = CpuV3Sim::default();
        let [broken0, broken1] = fpu_aux_raw(
            FpuAuxKind::IntegerRegister,
            1,
            0,
            63,
            FpuAuxSubop::Fld as u8,
            1,
        );
        machine
            .load_program(0, &[broken0, broken1, halt()])
            .unwrap();
        assert_eq!(
            machine.step(),
            Err(Fault {
                kind: FaultKind::InvalidInstruction,
                address: 0,
                instruction: broken0,
            })
        );
        assert_eq!(machine.pc(), 0);

        // VADD.4 with Fa = 61 needs F61..F64.
        let [word0, word1] = fpu_vector_raw(
            61,
            0,
            0,
            FpuVectorLength::Vec4 as u8,
            FpuVectorSubop::VAdd as u8,
            0,
        );
        let mut machine = CpuV3Sim::default();
        machine.load_program(0, &[word0, word1, halt()]).unwrap();
        assert_eq!(
            machine.step(),
            Err(Fault {
                kind: FaultKind::InvalidInstruction,
                address: 0,
                instruction: word0,
            })
        );
        assert_eq!(machine.pc(), 0);
    }

    #[test]
    fn fpu_compare_sets_the_existing_pending_test() {
        let mut program = vec![];
        program.extend(load_immediate16(0, 5));
        program.extend(load_immediate16(1, 9));
        program.extend(fpu_aux(
            FpuAuxKind::IntegerRegister,
            0,
            0,
            0,
            FpuAuxSubop::I16tof,
            0,
        )); // f0 = 5.0
        program.extend(fpu_aux(
            FpuAuxKind::IntegerRegister,
            1,
            0,
            1,
            FpuAuxSubop::I16tof,
            0,
        )); // f1 = 9.0
        program.extend(fpu_scalar(0, 1, 0, FpuScalarSubop::Cmp, 0));
        program.push(branch(TestCondition::LessThan, 1));
        program.push(halt());
        program.push(crate::move_register(0, 1));
        program.push(halt());
        let mut machine = CpuV3Sim::default();
        machine.load_program(0, &program).unwrap();
        assert_eq!(
            machine.run(64).unwrap(),
            RunOutcome::Halted {
                steps: 10,
                signal: 9
            }
        );
    }

    #[test]
    fn fpu_reserved_pairs_fault_and_rewind_to_the_first_word() {
        let mut machine = CpuV3Sim::default();
        // Reserved vector subop 0x10 in word1.
        machine.load_program(0, &[0xc000, 0x0080, halt()]).unwrap();
        assert_eq!(
            machine.step(),
            Err(Fault {
                kind: FaultKind::InvalidInstruction,
                address: 0,
                instruction: 0xc000
            })
        );
        assert_eq!(machine.pc(), 0);
        assert_eq!(machine.retired_words(), 0);

        // Reserved SINCOS mode 11.
        let mut machine = CpuV3Sim::default();
        machine.load_program(0, &[0xd000, 0x00e3, halt()]).unwrap();
        assert_eq!(
            machine.step(),
            Err(Fault {
                kind: FaultKind::InvalidInstruction,
                address: 0,
                instruction: 0xd000
            })
        );
        assert_eq!(machine.pc(), 0);

        // DOT keeps mode[2] reserved even when mode[1:0] names a stride.
        let [dot0, dot1] = fpu_vector_raw(
            0,
            0,
            0,
            FpuVectorLength::Vec2 as u8,
            FpuVectorSubop::Dot as u8,
            0b100,
        );
        let mut machine = CpuV3Sim::default();
        machine.load_program(0, &[dot0, dot1, halt()]).unwrap();
        assert_eq!(
            machine.step(),
            Err(Fault {
                kind: FaultKind::InvalidInstruction,
                address: 0,
                instruction: dot0
            })
        );
        assert_eq!(machine.pc(), 0);

        // A vector range past F63 faults before any register is written.
        let [far0, far1] = fpu_vector_raw(
            61,
            0,
            0,
            FpuVectorLength::Vec4 as u8,
            FpuVectorSubop::VAdd as u8,
            0,
        );
        let mut machine = CpuV3Sim::default();
        machine.load_program(0, &[far0, far1, halt()]).unwrap();
        assert_eq!(
            machine.step(),
            Err(Fault {
                kind: FaultKind::InvalidInstruction,
                address: 0,
                instruction: far0
            })
        );
        assert_eq!(machine.pc(), 0);
    }

    #[test]
    fn revision_three_comparisons_cover_overflow_edges() {
        let mut program = vec![];
        program.extend(load_immediate16(1, 0x8000));
        program.extend(load_immediate16(2, 0x7fff));
        program.extend([
            crate::move_register(3, 1),
            set_less_than_signed(3, 2),
            crate::move_register(4, 1),
            set_less_than_unsigned(4, 2),
            population_count(5, 1),
            halt(),
        ]);
        let mut machine = CpuV3Sim::default();
        machine.load_program(0, &program).unwrap();
        machine.run(20).unwrap();

        assert_eq!(machine.register(3), Some(1));
        assert_eq!(machine.register(4), Some(0));
        assert_eq!(machine.register(5), Some(1));
    }

    #[test]
    fn boot_code_establishes_fixed_segments_and_enters_an_application() {
        let mut boot = vec![];
        boot.extend(load_immediate16(1, 1));
        boot.extend(load_immediate16(2, 0x0020));
        boot.extend(load_immediate16(3, 2));
        boot.extend([write_data_segment(3), jump_segment(1, 2)]);

        let mut application = vec![
            read_special(4, SpecialRegister::CodeSegment),
            read_special(5, SpecialRegister::DataSegment),
        ];
        application.extend(load_immediate16(6, 0x1234));
        application.extend([load(0, 6, 0), halt()]);

        let mut machine = CpuV3Sim::default();
        machine.load_program(0, &boot).unwrap();
        machine.load_segment(1, 0x0020, &application).unwrap();
        machine.load_segment(2, 0x1234, &[0xbeef]).unwrap();

        assert_eq!(
            machine.run(32).unwrap(),
            RunOutcome::Halted {
                steps: 14,
                signal: 0xbeef,
            }
        );
        assert_eq!(machine.code_segment(), 1);
        assert_eq!(machine.data_segment(), 2);
        assert_eq!(machine.register(4), Some(1));
        assert_eq!(machine.register(5), Some(2));
    }

    #[test]
    fn fitted_memory_rejects_an_unimplemented_segment() {
        let mut program = vec![];
        program.extend(load_immediate16(1, 1));
        program.extend([write_data_segment(1), load(0, 0, 0)]);

        let mut machine = CpuV3Sim::with_physical_memory_words(1 << 16);
        machine.load_program(0, &program).unwrap();
        assert_eq!(
            machine.run(8),
            Err(Fault {
                kind: FaultKind::PhysicalAddressOutOfRange {
                    address: PhysicalWordAddress::new(0x0001_0000),
                },
                address: 3,
                instruction: load(0, 0, 0),
            })
        );
    }

    #[test]
    fn high_offsets_are_ordinary_memory_in_the_selected_data_segment() {
        let mut program = vec![];
        program.extend(load_immediate16(1, 3));
        program.extend(load_immediate16(2, 0xff00));
        program.extend(load_immediate16(3, 0x55aa));
        program.extend([write_data_segment(1), store(3, 2, 0), load(0, 2, 0), halt()]);

        let mut machine = CpuV3Sim::default();
        machine.load_program(0, &program).unwrap();
        machine.attach_device(0, Box::new(EchoDevice { channels: [0; 16] }));
        assert_eq!(
            machine.run(24).unwrap(),
            RunOutcome::Halted {
                steps: 10,
                signal: 0x55aa
            }
        );
        assert_eq!(
            machine.physical_memory(PhysicalWordAddress::new(0x0003_ff00)),
            0x55aa
        );
        assert_eq!(machine.memory(0xff00), 0);
        assert_eq!(machine.device::<EchoDevice>(0).unwrap().channels[0], 0);
    }

    #[test]
    fn all_six_predicates_test_the_pending_ordering() {
        for (condition, taken) in [
            (TestCondition::Equal, false),
            (TestCondition::NotEqual, true),
            (TestCondition::LessThan, true),
            (TestCondition::GreaterOrEqual, false),
            (TestCondition::GreaterThan, false),
            (TestCondition::LessOrEqual, true),
        ] {
            // r1 = 3, r2 = 5: the signed pending ordering is Less.
            let mut program = vec![];
            program.extend(load_immediate16(1, 3));
            program.extend(load_immediate16(2, 5));
            program.extend([
                compare_signed(1, 2),
                branch(condition, 1),
                immediate_unsigned(ImmediateOp::LoadUnsigned, 0, 9),
                halt(),
            ]);
            let mut machine = CpuV3Sim::default();
            machine.load_program(0, &program).unwrap();
            let expected = if taken { 0 } else { 9 };
            assert_eq!(
                machine.run(16).unwrap(),
                RunOutcome::Halted {
                    steps: if taken { 7 } else { 8 },
                    signal: expected,
                },
                "condition {condition:?}"
            );
        }
    }

    #[test]
    fn branch_offset_is_a_signed_byte_relative_to_the_next_word() {
        assert_eq!(branch(TestCondition::Equal, -128), 0xb080);
        assert_eq!(branch(TestCondition::Equal, 127), 0xb07f);

        // r1 = 0 -> pending Equal; the taken branch skips two words.
        let mut program = vec![];
        program.extend(load_immediate16(1, 0));
        program.extend([
            compare_signed(1, 1),
            branch(TestCondition::Equal, 2),
            immediate_unsigned(ImmediateOp::LoadUnsigned, 0, 1),
            immediate_unsigned(ImmediateOp::LoadUnsigned, 0, 2),
            halt(),
        ]);
        let mut machine = CpuV3Sim::default();
        machine.load_program(0, &program).unwrap();
        assert_eq!(
            machine.run(16).unwrap(),
            RunOutcome::Halted {
                steps: 5,
                signal: 0,
            }
        );
    }

    #[test]
    fn prefixed_branch_forms_a_wide_offset_and_retires_two_words() {
        let mut program = vec![immediate_signed(ImmediateOp::CompareSigned, 0, 0)];
        program.extend(prefixed_branch(branch(TestCondition::Equal, 0), 0x0103));
        assert_eq!(program[1], prefix12(0x01));
        assert_eq!(program[2], 0xb003);
        let filler = immediate_unsigned(ImmediateOp::LoadUnsigned, 1, 1);
        program.extend(std::iter::repeat_n(filler, 0x103));
        program.extend([immediate_unsigned(ImmediateOp::LoadUnsigned, 1, 2), halt()]);

        let mut machine = CpuV3Sim::default();
        machine.load_program(0, &program).unwrap();
        assert_eq!(
            machine.run(0x200).unwrap(),
            RunOutcome::Halted {
                steps: 5,
                signal: 0,
            }
        );
        // The branch target executed exactly once; the filler did not run.
        assert_eq!(machine.register(1), Some(2));
        assert_eq!(machine.retired_words(), 5);
    }

    #[test]
    fn conditional_branch_without_a_pending_test_faults() {
        let mut machine = CpuV3Sim::default();
        machine
            .load_program(0, &[branch(TestCondition::Equal, 0)])
            .unwrap();
        assert_eq!(
            machine.step(),
            Err(Fault {
                kind: FaultKind::InvalidInstruction,
                address: 0,
                instruction: 0xb000,
            })
        );
    }

    #[test]
    fn stale_branch_fault_reports_the_prefix_address_and_retires_nothing() {
        let mut machine = CpuV3Sim::default();
        let words = prefixed_branch(branch(TestCondition::NotEqual, 0), 0);
        machine.load_program(0, &words).unwrap();
        assert_eq!(machine.step(), Ok(StepOutcome::Running));
        assert_eq!(
            machine.step(),
            Err(Fault {
                kind: FaultKind::InvalidInstruction,
                address: 0,
                instruction: words[1],
            })
        );
        assert_eq!(machine.retired_words(), 0);
    }

    #[test]
    fn reserved_encodings_fault() {
        // Majors C/D/E are now FPU v2 first words, so only the non-canonical
        // JREG/JALR link fields remain invalid single words.
        for word in [0xbe10, 0xbf00] {
            let mut machine = CpuV3Sim::default();
            machine.load_program(0, &[word]).unwrap();
            assert_eq!(
                machine.step(),
                Err(Fault {
                    kind: FaultKind::InvalidInstruction,
                    address: 0,
                    instruction: word,
                }),
                "word {word:#06x}"
            );
        }
    }

    #[test]
    fn conditional_move_consumes_the_pending_test() {
        // r1 = 3, r2 = 5: signed pending ordering is Less; MOVLT writes,
        // MOVGE does not but still consumes the test.
        let mut program = vec![];
        program.extend(load_immediate16(1, 3));
        program.extend(load_immediate16(2, 5));
        program.extend(load_immediate16(3, 0));
        program.extend([
            compare_signed(1, 2),
            conditional_move(TestCondition::LessThan, 3, 2),
            compare_signed(1, 2),
            conditional_move(TestCondition::GreaterOrEqual, 3, 1),
            move_register(0, 3),
            halt(),
        ]);
        let mut machine = CpuV3Sim::default();
        machine.load_program(0, &program).unwrap();
        assert_eq!(
            machine.run(32).unwrap(),
            RunOutcome::Halted {
                steps: 12,
                signal: 5
            }
        );

        let mut machine = CpuV3Sim::default();
        machine
            .load_program(0, &[conditional_move(TestCondition::Equal, 1, 2)])
            .unwrap();
        assert_eq!(
            machine.step(),
            Err(Fault {
                kind: FaultKind::InvalidInstruction,
                address: 0,
                instruction: conditional_move(TestCondition::Equal, 1, 2),
            })
        );
    }

    #[test]
    fn nonzero_signal_types_retire_as_events() {
        let mut program = vec![];
        program.extend(load_immediate16(1, 0x77));
        program.extend([signal(1, 1), signal(1, 15), move_register(0, 1), halt()]);
        let mut machine = CpuV3Sim::default();
        machine.load_program(0, &program).unwrap();
        // Each nonzero SIGNAL surfaces exactly once at its retirement edge and
        // execution continues; step() observes the events, run() ignores them.
        assert_eq!(machine.step(), Ok(StepOutcome::Running));
        assert_eq!(machine.step(), Ok(StepOutcome::Running));
        assert_eq!(
            machine.step(),
            Ok(StepOutcome::Signaled(SignalEvent {
                signal_type: 1,
                value: 0x77
            }))
        );
        assert_eq!(machine.retired_words(), 3);
        assert_eq!(
            machine.step(),
            Ok(StepOutcome::Signaled(SignalEvent {
                signal_type: 15,
                value: 0x77
            }))
        );

        let mut machine = CpuV3Sim::default();
        machine.load_program(0, &program).unwrap();
        assert_eq!(
            machine.run(16).unwrap(),
            RunOutcome::Halted {
                steps: 6,
                signal: 0x77
            }
        );
        assert_eq!(machine.retired_words(), 6);
    }

    #[test]
    fn signal_type_zero_latches_the_selected_register() {
        // HALT is SIGNAL r0, 0; a nonzero source register halts with its value.
        let mut program = vec![];
        program.extend(load_immediate16(1, 0x1234));
        program.extend([signal(1, 0)]);
        let mut machine = CpuV3Sim::default();
        machine.load_program(0, &program).unwrap();
        assert_eq!(
            machine.run(8).unwrap(),
            RunOutcome::Halted {
                steps: 3,
                signal: 0x1234
            }
        );
        // After the halt the same latched signal is re-reported.
        assert_eq!(machine.step(), Ok(StepOutcome::Halted { signal: 0x1234 }));
    }

    #[test]
    fn prefixes_are_transparent_to_the_pending_test() {
        // CMP; PFX12; BR: the prefix sits between producer and consumer.
        let mut machine = CpuV3Sim::default();
        machine
            .load_program(
                0,
                &[
                    immediate_signed(ImmediateOp::CompareSigned, 0, 0),
                    prefix12(0),
                    branch(TestCondition::Equal, 1),
                    immediate_unsigned(ImmediateOp::LoadUnsigned, 0, 9),
                    halt(),
                ],
            )
            .unwrap();
        assert_eq!(
            machine.run(8).unwrap(),
            RunOutcome::Halted {
                steps: 4,
                signal: 0,
            }
        );
        assert_eq!(machine.retired_words(), 4);
    }

    #[test]
    fn unconditional_jumps_expire_the_pending_test() {
        let mut machine = CpuV3Sim::default();
        machine
            .load_program(
                0,
                &[
                    immediate_signed(ImmediateOp::CompareSigned, 0, 0),
                    jump_relative(2),
                    nop(),
                    nop(),
                    branch(TestCondition::Equal, 0),
                ],
            )
            .unwrap();
        assert_eq!(machine.step(), Ok(StepOutcome::Running));
        assert_eq!(machine.step(), Ok(StepOutcome::Running));
        assert_eq!(machine.pc(), 4);
        assert_eq!(
            machine.step(),
            Err(Fault {
                kind: FaultKind::InvalidInstruction,
                address: 4,
                instruction: 0xb000,
            })
        );
    }

    #[test]
    fn jump_and_link_relative_links_the_fall_through_address() {
        let mut machine = CpuV3Sim::default();
        machine
            .load_program(0, &[jump_and_link_relative(2), nop(), nop(), halt()])
            .unwrap();
        assert_eq!(
            machine.run(4).unwrap(),
            RunOutcome::Halted {
                steps: 2,
                signal: 0,
            }
        );
        assert_eq!(machine.register(LINK_REGISTER), Some(1));
        assert_eq!(machine.retired_words(), 2);
    }

    #[test]
    fn jump_and_link_register_requires_the_fixed_link_register() {
        let mut machine = CpuV3Sim::default();
        machine.load_program(0, &[0xbf51]).unwrap();
        assert_eq!(
            machine.step(),
            Err(Fault {
                kind: FaultKind::InvalidInstruction,
                address: 0,
                instruction: 0xbf51,
            })
        );

        let mut machine = CpuV3Sim::default();
        machine
            .load_program(
                0,
                &[
                    immediate_unsigned(ImmediateOp::LoadUnsigned, 2, 3),
                    jump_and_link_register(2),
                    halt(),
                    halt(),
                ],
            )
            .unwrap();
        assert_eq!(
            machine.run(4).unwrap(),
            RunOutcome::Halted {
                steps: 3,
                signal: 0,
            }
        );
        assert_eq!(machine.register(LINK_REGISTER), Some(2));
    }

    #[test]
    fn compare_instructions_respect_signedness_at_the_sign_boundary() {
        // r1 = 0x7fff (signed 32767), r2 = 0x8000 (signed -32768, unsigned 32768).
        let mut program = vec![];
        program.extend(load_immediate16(1, 0x7fff));
        program.extend(load_immediate16(2, 0x8000));
        // r3: signed register compare -> Greater.
        program.extend([
            immediate_unsigned(ImmediateOp::LoadUnsigned, 3, 1),
            compare_signed(1, 2),
            branch(TestCondition::GreaterThan, 1),
            immediate_unsigned(ImmediateOp::LoadUnsigned, 3, 0),
        ]);
        // r4: unsigned register compare -> Less.
        program.extend([
            immediate_unsigned(ImmediateOp::LoadUnsigned, 4, 1),
            compare_unsigned(1, 2),
            branch(TestCondition::LessThan, 1),
            immediate_unsigned(ImmediateOp::LoadUnsigned, 4, 0),
        ]);
        // r5: signed immediate compare against the 16-bit pattern 0x8000
        // (i16 -32768) -> Greater.
        program.push(immediate_unsigned(ImmediateOp::LoadUnsigned, 5, 1));
        program.extend(prefixed(
            0xa000 | ((ImmediateOp::CompareSigned as Word) << 8) | 0x10,
            0x8000,
        ));
        program.extend([
            branch(TestCondition::GreaterThan, 1),
            immediate_unsigned(ImmediateOp::LoadUnsigned, 5, 0),
        ]);
        // r6: unsigned immediate compare against 0x8000 -> Less.
        program.push(immediate_unsigned(ImmediateOp::LoadUnsigned, 6, 1));
        program.extend(prefixed(
            0xa000 | ((ImmediateOp::CompareUnsigned as Word) << 8) | 0x10,
            0x8000,
        ));
        program.extend([
            branch(TestCondition::LessThan, 1),
            immediate_unsigned(ImmediateOp::LoadUnsigned, 6, 0),
        ]);
        // r7/r8: unprefixed immediate compares sign-extend the nibble.
        program.extend([immediate_signed(ImmediateOp::LoadSigned, 1, -1)]);
        program.extend([
            immediate_unsigned(ImmediateOp::LoadUnsigned, 7, 1),
            immediate_signed(ImmediateOp::CompareSigned, 1, 0),
            branch(TestCondition::LessThan, 1),
            immediate_unsigned(ImmediateOp::LoadUnsigned, 7, 0),
        ]);
        program.extend([
            immediate_unsigned(ImmediateOp::LoadUnsigned, 8, 1),
            immediate_unsigned(ImmediateOp::CompareUnsigned, 1, 0),
            branch(TestCondition::GreaterThan, 1),
            immediate_unsigned(ImmediateOp::LoadUnsigned, 8, 0),
        ]);
        program.push(halt());

        let mut machine = CpuV3Sim::default();
        machine.load_program(0, &program).unwrap();
        assert!(matches!(
            machine.run(100).unwrap(),
            RunOutcome::Halted { .. }
        ));
        for (register, name) in [
            (3, "CMPS"),
            (4, "CMPU"),
            (5, "CMPSI wide"),
            (6, "CMPUI wide"),
            (7, "CMPSI"),
            (8, "CMPUI"),
        ] {
            assert_eq!(machine.register(register), Some(1), "{name}");
        }
        // CMP-class instructions write no register: r1/r2 keep their values.
        assert_eq!(machine.register(2), Some(0x8000));
    }

    #[test]
    fn shift_amounts_are_masked_to_four_bits() {
        // Register-count shifts use rs & 15: amounts 0/15/16/31 exercise the
        // mask boundary, and the high 12 bits of rs are ignored entirely.
        let mut program = vec![];
        for (slot, amount) in [0u16, 15, 16, 31].into_iter().enumerate() {
            program.extend(load_immediate16(2, amount | 0x8000));
            program.extend(load_immediate16(3, 0x0003));
            program.extend([
                shift_register(ShiftOp::Left, 3, 2),
                move_register((4 + slot) as u8, 3),
            ]);
        }
        program.push(halt());
        let mut machine = CpuV3Sim::default();
        machine.load_program(0, &program).unwrap();
        machine.run(64).unwrap();
        // 3 << 0 == 3, 3 << 15 == 0x8000; 16 and 31 mask back to 0 and 15.
        for (slot, expected) in [0x0003u16, 0x8000, 0x0003, 0x8000].into_iter().enumerate() {
            assert_eq!(
                machine.register((4 + slot) as u8),
                Some(expected),
                "slot {slot}"
            );
        }
    }

    #[test]
    fn destructive_read_modify_write_uses_the_old_value() {
        // rd == rs: the destructive form reads the old rd value before
        // writing the result back into the same register.
        let mut program = vec![];
        program.extend(load_immediate16(1, 0x00ff));
        program.extend([
            shift_register(ShiftOp::Left, 1, 1), // 0xff << 15 = 0x8000
            multiply(MultiplyWindow::Low, 1, 1), // 0x8000^2 low = 0
        ]);
        program.extend(load_immediate16(2, 0x00ff));
        program.extend([
            // 0xff * 0xff = 0xfe01: MUL8 keeps [23:8] = 0xfe, MUL16 keeps 0.
            multiply(MultiplyWindow::Shift8, 2, 2),
        ]);
        program.extend(load_immediate16(3, 0xffff));
        program.extend([
            // 0xffff * 0xffff = 0xfffe0001: MUL16 keeps 0xfffe.
            multiply(MultiplyWindow::Shift16, 3, 3),
        ]);
        program.extend([move_register(0, 1), halt()]);
        let mut machine = CpuV3Sim::default();
        machine.load_program(0, &program).unwrap();
        machine.run(32).unwrap();
        assert_eq!(machine.register(1), Some(0));
        assert_eq!(machine.register(2), Some(0x00fe));
        assert_eq!(machine.register(3), Some(0xfffe));
    }

    #[test]
    fn multiply_windows_select_product_bytes() {
        let mut program = vec![];
        program.extend(load_immediate16(1, 0xffff));
        program.extend(load_immediate16(2, 0xffff));
        // 0xffff * 0xffff = 0xfffe0001.
        program.extend([multiply(MultiplyWindow::Low, 2, 1)]); // [15:0]
        program.extend(load_immediate16(3, 0xffff));
        program.extend([multiply(MultiplyWindow::Shift8, 3, 1)]); // [23:8]
        program.extend(load_immediate16(4, 0xffff));
        program.extend([multiply(MultiplyWindow::Shift16, 4, 1)]); // [31:16]
        program.push(halt());
        let mut machine = CpuV3Sim::default();
        machine.load_program(0, &program).unwrap();
        machine.run(32).unwrap();
        assert_eq!(machine.register(2), Some(0x0001));
        assert_eq!(machine.register(3), Some(0xfe00));
        assert_eq!(machine.register(4), Some(0xfffe));
    }

    #[test]
    fn muli_immediate_is_an_unsigned_bit_pattern() {
        // Short form: 15 is fifteen, not minus one.
        let mut program = vec![];
        program.extend(load_immediate16(1, 2));
        program.push(multiply_immediate(1, 15));
        // Wide form: the prefixed pattern 0x8000 is unsigned 32768.
        program.extend(load_immediate16(2, 2));
        program.extend(prefixed(multiply_immediate(2, 0), 0x8000));
        program.push(halt());
        let mut machine = CpuV3Sim::default();
        machine.load_program(0, &program).unwrap();
        machine.run(16).unwrap();
        assert_eq!(machine.register(1), Some(30));
        assert_eq!(machine.register(2), Some(0));
    }

    #[test]
    fn all_six_conditional_moves_test_the_pending_ordering() {
        for (condition, writes) in [
            (TestCondition::Equal, false),
            (TestCondition::NotEqual, true),
            (TestCondition::LessThan, true),
            (TestCondition::GreaterOrEqual, false),
            (TestCondition::GreaterThan, false),
            (TestCondition::LessOrEqual, true),
        ] {
            // r1 = 3, r2 = 5: the signed pending ordering is Less.
            let mut program = vec![];
            program.extend(load_immediate16(1, 3));
            program.extend(load_immediate16(2, 5));
            program.extend(load_immediate16(3, 0));
            program.extend([
                compare_signed(1, 2),
                conditional_move(condition, 3, 2),
                move_register(0, 3),
                halt(),
            ]);
            let mut machine = CpuV3Sim::default();
            machine.load_program(0, &program).unwrap();
            let expected = if writes { 5 } else { 0 };
            assert_eq!(
                machine.run(16).unwrap(),
                RunOutcome::Halted {
                    steps: 10,
                    signal: expected,
                },
                "condition {condition:?}"
            );
        }
    }

    #[test]
    fn conditional_move_consumes_pending_even_when_not_taken() {
        let mut program = vec![];
        program.extend(load_immediate16(1, 3));
        program.extend(load_immediate16(2, 5));
        program.extend([
            compare_signed(1, 2),
            // Not taken (the ordering is Less), but the pending test is gone.
            conditional_move(TestCondition::GreaterOrEqual, 1, 2),
            branch(TestCondition::Equal, 0),
        ]);
        let mut machine = CpuV3Sim::default();
        machine.load_program(0, &program).unwrap();
        assert_eq!(
            machine.run(16),
            Err(Fault {
                kind: FaultKind::InvalidInstruction,
                address: 6,
                instruction: branch(TestCondition::Equal, 0),
            })
        );
    }

    #[test]
    fn jalr_and_jalrel_link_r14_and_jreg_returns() {
        // JALR path: call the subroutine at offset 8, which returns via
        // JREG r14 with the call's fall-through address in r0.
        let mut program = vec![];
        program.extend(load_immediate16(2, 8));
        program.push(jump_and_link_register(2));
        program.push(halt()); // returns here, halting with r0
        program.extend([nop(); 4]);
        program.extend([
            move_register(0, LINK_REGISTER),
            jump_register(LINK_REGISTER),
        ]);
        let mut machine = CpuV3Sim::default();
        machine.load_program(0, &program).unwrap();
        assert_eq!(
            machine.run(16).unwrap(),
            RunOutcome::Halted {
                steps: 6,
                signal: 3
            }
        );
        assert_eq!(machine.register(LINK_REGISTER), Some(3));

        // JALREL path: same flow through a relative call.
        let program = [
            jump_and_link_relative(2),
            move_register(0, LINK_REGISTER),
            halt(),
            jump_register(LINK_REGISTER),
        ];
        let mut machine = CpuV3Sim::default();
        machine.load_program(0, &program).unwrap();
        assert_eq!(
            machine.run(8).unwrap(),
            RunOutcome::Halted {
                steps: 4,
                signal: 1
            }
        );
    }

    #[test]
    fn immediate_shifts_cover_the_extreme_amounts() {
        let mut program = vec![];
        program.extend(load_immediate16(1, 0x8001));
        program.extend([
            shift_immediate(ShiftOp::Left, 1, 0),             // unchanged
            shift_immediate(ShiftOp::RightLogical, 1, 15),    // 0x8001 >> 15 = 1
            shift_immediate(ShiftOp::Left, 1, 15),            // 1 << 15 = 0x8000
            shift_immediate(ShiftOp::RightArithmetic, 1, 15), // sign fills: 0xffff
            move_register(0, 1),
            halt(),
        ]);
        let mut machine = CpuV3Sim::default();
        machine.load_program(0, &program).unwrap();
        assert_eq!(
            machine.run(16).unwrap(),
            RunOutcome::Halted {
                steps: 8,
                signal: 0xffff
            }
        );
    }

    struct EchoDevice {
        channels: [Word; 16],
    }

    impl Device for EchoDevice {
        fn read(&mut self, _memory: &mut [Word], channel: u8) -> Word {
            self.channels[usize::from(channel)]
        }

        fn write(&mut self, _memory: &mut [Word], channel: u8, value: Word) {
            self.channels[usize::from(channel)] = value;
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    fn device_round_trip_program() -> Vec<Word> {
        let mut program = vec![];
        program.extend(load_immediate16(1, 0x1234));
        program.extend([device_send(1, 2, 3), device_receive(0, 2, 3), halt()]);
        program
    }

    #[test]
    fn device_instructions_route_to_an_attached_device() {
        let mut machine = CpuV3Sim::default();
        machine
            .load_program(0, &device_round_trip_program())
            .unwrap();
        machine.attach_device(2, Box::new(EchoDevice { channels: [0; 16] }));
        assert_eq!(
            machine.run(16).unwrap(),
            RunOutcome::Halted {
                steps: 5,
                signal: 0x1234,
            }
        );
        let device: &EchoDevice = machine.device(2).unwrap();
        assert_eq!(device.channels[3], 0x1234);
        // Device traffic never aliases an ordinary physical memory word.
        assert_eq!(machine.memory(0xff23), 0);
    }

    #[test]
    fn unconnected_device_reads_zero_and_writes_are_ignored() {
        let mut machine = CpuV3Sim::default();
        machine
            .load_program(0, &device_round_trip_program())
            .unwrap();
        assert_eq!(
            machine.run(16).unwrap(),
            RunOutcome::Halted {
                steps: 5,
                signal: 0,
            }
        );
        assert_eq!(machine.memory(0xff23), 0);
    }
}
