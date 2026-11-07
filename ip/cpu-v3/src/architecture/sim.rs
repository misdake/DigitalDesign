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
    UnalignedAddress { address: PhysicalWordAddress },
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
    /// Optional CPU-local hold driven by a device-channel value watch.
    fn cpu_hold_watch(&self) -> Option<(u8, u8, Word)> {
        None
    }
    /// Supplies one sampled value to the active hold owner.
    fn observe_cpu_hold(&mut self, _value: Word) {}
}

/// A pending DEVSEND, before architectural state or the device is changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeviceWrite {
    pub device: u8,
    pub channel: u8,
    pub value: Word,
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
    data_segments: [Word; 4],
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
    /// Construct a board cold state with a fixed entry segment. The generic
    /// CPU and all existing benchmark constructors retain segment zero.
    pub fn with_initial_code_segment(words: usize, segment: Word) -> Self {
        let mut cpu = Self::with_physical_memory_words(words);
        cpu.code_segment = segment;
        cpu
    }

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
            data_segments: [0, 1, 2, 3],
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

    /// Borrowed physical storage for shared renderers and observation tools.
    pub fn physical_memory_slice(&self) -> &[Word] {
        &self.memory
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

    /// Performs one device read without retiring a CPU instruction so an
    /// autonomous target can advance while a watch holds the CPU.
    fn cycle_device_read(&mut self, device: u8, channel: u8) -> Word {
        let (devices, memory) = (&mut self.devices, &mut self.memory);
        devices
            .get_mut(usize::from(device))
            .and_then(Option::as_mut)
            .map_or(0, |handler| handler.read(memory, channel))
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
        self.data_segments[0] >> 2
    }

    pub fn data_segments(&self) -> [Word; 4] {
        self.data_segments
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
        Ok(self
            .step_observing(|_, _| true)?
            .expect("unconditional step advances"))
    }
    /// A false result pauses before DEVSEND without changing PC, retirement,
    /// pending prefix/test or physical memory. Existing device holds still run.
    pub fn step_observing(
        &mut self,
        mut before_write: impl FnMut(DeviceWrite, &[Word]) -> bool,
    ) -> Result<Option<StepOutcome>, Fault> {
        if self.halted {
            return Ok(Some(StepOutcome::Halted {
                signal: self.halt_signal,
            }));
        }

        if let Some((owner, device, channel)) =
            self.devices
                .iter()
                .enumerate()
                .find_map(|(owner, handler)| {
                    handler
                        .as_ref()
                        .and_then(|handler| handler.cpu_hold_watch())
                        .map(|(device, channel, _)| (owner, device, channel))
                })
        {
            let value = self.cycle_device_read(device, channel);
            if let Some(handler) = self.devices[owner].as_mut() {
                handler.observe_cpu_hold(value);
            }
            return Ok(Some(StepOutcome::Running));
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
        if instruction >> 12 == 7 && instruction & 0x800 != 0 {
            let write = DeviceWrite {
                device: ((instruction >> 8) & 7) as u8,
                channel: ((instruction >> 4) & 15) as u8,
                value: self.registers[usize::from(instruction & 15)],
            };
            if !before_write(write, &self.memory) {
                return Ok(None);
            }
        }
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
            return Ok(Some(StepOutcome::Running));
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
                    Ok(Some(outcome))
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
                Ok(Some(outcome))
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
            FpuVectorSubop::VAbs | FpuVectorSubop::VNeg | FpuVectorSubop::VMove
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
            7 => {
                let offset = self.registers[usize::from(dst)];
                let destination_pointer = self.registers[usize::from(src)];
                let source = self.data_address(offset);
                if offset & 0x0f != 0 {
                    return Err(FaultKind::UnalignedAddress { address: source });
                }
                let destination = self.data_address(destination_pointer);
                if destination_pointer & 15 != 0 {
                    return Err(FaultKind::UnalignedAddress {
                        address: destination,
                    });
                }
                let source_start = source.get() as usize;
                let destination_start = destination.get() as usize;
                let source_end = source_start
                    .checked_add(16)
                    .ok_or(FaultKind::PhysicalAddressOutOfRange { address: source })?;
                let destination_end = destination_start.checked_add(16).ok_or(
                    FaultKind::PhysicalAddressOutOfRange {
                        address: destination,
                    },
                )?;
                if source_end > self.memory.len() {
                    return Err(FaultKind::PhysicalAddressOutOfRange { address: source });
                }
                if destination_end > self.memory.len() {
                    return Err(FaultKind::PhysicalAddressOutOfRange {
                        address: destination,
                    });
                }
                let mut line = [0; 16];
                line.copy_from_slice(&self.memory[source_start..source_end]);
                self.memory[destination_start..destination_end].copy_from_slice(&line);
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
                    1 => self.data_segments[0] >> 2,
                    2..=5 => self.data_segments[usize::from(src - 2)],
                    _ => return Err(FaultKind::InvalidInstruction),
                }
            }
            14 if dst == 1 => {
                let value = self.registers[usize::from(src)];
                if value & !0x3f != 0 {
                    return Err(FaultKind::PhysicalAddressOutOfRange {
                        address: self.data_address(0),
                    });
                }
                let base = value << 2;
                self.data_segments = [base, base + 1, base + 2, base + 3];
            }
            14 if (2..=5).contains(&dst) => {
                let value = self.registers[usize::from(src)];
                if value & !0xff != 0 {
                    return Err(FaultKind::PhysicalAddressOutOfRange {
                        address: self.data_address(0),
                    });
                }
                self.data_segments[usize::from(dst - 2)] = value;
            }
            // Cache-only commands have no architectural-memory effect or
            // asynchronous latency in the naive interpreter. DWAIT therefore
            // completes immediately; command timing/errors belong to the
            // cycle model and RTL.
            14 if matches!(dst, 6 | 8) => {}
            15 => {
                self.code_segment = self.registers[usize::from(dst)];
                self.pc = self.registers[usize::from(src)];
            }
            _ => return Err(FaultKind::InvalidInstruction),
        }
        Ok(StepOutcome::Running)
    }

    fn data_address(&self, offset: Word) -> PhysicalWordAddress {
        let page = self.data_segments[usize::from(offset >> 14)];
        PhysicalWordAddress::new((u32::from(page) << 14) | u32::from(offset & 0x3fff))
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
#[path = "sim_tests/mod.rs"]
mod tests;
