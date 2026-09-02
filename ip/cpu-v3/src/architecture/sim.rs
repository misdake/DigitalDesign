//! Small architectural interpreter used as the CpuV3 correctness oracle.

use super::encoding::{is_prefix_consumer, sign_extend, SpecialRegister, Word, LINK_REGISTER};
use super::{
    acc_saturate, fix16_abs, fix16_add, fix16_ceil, fix16_compare, fix16_floor, fix16_from_acc,
    fix16_mul, fix16_neg, fix16_reciprocal, fix16_reciprocal_sqrt, fix16_round, fix16_saturate01,
    fix16_sign, fix16_sin_cos, fix16_sub, FpuDomainError, FpuUnaryOp, FpuVector,
    PhysicalWordAddress,
};
use std::cmp::Ordering;

/// Fitted 8-MiB SDRAM on the initial Tang Nano 20K target.
pub const DEFAULT_PHYSICAL_MEMORY_WORDS: usize = 1 << 22;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FaultKind {
    InvalidInstruction,
    FpuDomain(FpuDomainError),
    MisalignedFpuVectorAddress { offset: Word },
    PhysicalAddressOutOfRange { address: PhysicalWordAddress },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Fault {
    pub kind: FaultKind,
    pub address: Word,
    pub instruction: Word,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StepOutcome {
    Running,
    Halted { signal: Word },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Prefix {
    address: Word,
    high: Word,
}

/// A device reached exclusively through DEVRECV and DEVSEND. `memory` is the
/// complete physical memory, so DMA-style devices can move data.
pub trait Device {
    fn read(&mut self, memory: &mut [Word], channel: u8) -> Word;
    fn write(&mut self, memory: &mut [Word], channel: u8, value: Word);
    /// Downcast support for test assertions on attached models.
    fn as_any(&self) -> &dyn std::any::Any;
}

pub struct Machine {
    registers: [Word; 16],
    fpu_registers: [FpuVector; 16],
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
}

impl Default for Machine {
    fn default() -> Self {
        Self::with_physical_memory_words(DEFAULT_PHYSICAL_MEMORY_WORDS)
    }
}

impl Machine {
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
            fpu_registers: [[0; 4]; 16],
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
        }
    }
}

impl Machine {
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

    pub fn fpu_registers(&self) -> &[FpuVector; 16] {
        &self.fpu_registers
    }

    pub fn fpu_register(&self, index: u8) -> Option<FpuVector> {
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
                signal: self.registers[0],
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
                high: instruction & 0xfff,
            });
            return Ok(StepOutcome::Running);
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
            0x0..=0x7 => self.execute_alu(opcode, instruction),
            0x8 => self.execute_load(instruction, prefix),
            0x9 => self.execute_store(instruction, prefix),
            0xa => self.execute_immediate(instruction, prefix),
            0xb => self.execute_branch(instruction, prefix, pending),
            0xc => self.execute_device(instruction),
            0xd => self.execute_fpu(instruction),
            0xe => self.execute_control(instruction),
            _ => unreachable!(),
        };
        match result {
            Ok(outcome) => {
                self.retired_words += retire_words;
                Ok(outcome)
            }
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
            2 => lhs.wrapping_mul(rhs),
            3 => lhs & rhs,
            4 => lhs | rhs,
            5 => lhs ^ rhs,
            6 => lhs.wrapping_shl(u32::from(rhs & 15)),
            7 => ((lhs as i16) >> u32::from(rhs & 15)) as Word,
            _ => unreachable!(),
        };
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
        let signed = immediate4(instruction, prefix, true);
        let unsigned = immediate4(instruction, prefix, false);
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
            0 => old.wrapping_add(signed),
            1 => old.wrapping_sub(signed),
            2 => old & unsigned,
            3 => old | unsigned,
            4 => old ^ unsigned,
            5 => old.wrapping_shl(u32::from(instruction & 15)),
            6 => old.wrapping_shr(u32::from(instruction & 15)),
            7 => ((old as i16) >> u32::from(instruction & 15)) as Word,
            8 => old.wrapping_mul(signed),
            9 => Word::from(old == signed),
            10 => Word::from((old as i16) < (signed as i16)),
            11 => Word::from(old < unsigned),
            14 if prefix.is_some() => unsigned,
            14 => sign_extend(instruction & 15, 4),
            15 => unsigned,
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
        let condition = field(instruction, 8);
        let offset = prefix.map_or_else(
            || sign_extend(instruction & 0xff, 8),
            |prefix| ((prefix.high & 0xff) << 8) | (instruction & 0xff),
        );
        match condition {
            // Conditional branches consume the pending test result, whether
            // or not the branch is taken.
            0..=5 => {
                let test = pending.ok_or(FaultKind::InvalidInstruction)?;
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
            // JALREL: link the fall-through address into r14, then jump.
            9 => {
                let next = self.pc;
                self.pc = next.wrapping_add(offset);
                self.registers[usize::from(LINK_REGISTER)] = next;
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

    fn execute_fpu(&mut self, instruction: Word) -> ExecuteResult {
        let function = field(instruction, 8);
        let a = usize::from(field(instruction, 4));
        let b = usize::from(field(instruction, 0));
        match function {
            0 => self.fpu_registers[a] = [self.registers[b] as i16, 0, 0, 0],
            1 => self.registers[a] = self.fpu_registers[b][0] as Word,
            2 => {
                let offset = self.registers[b];
                if offset & 3 != 0 {
                    return Err(FaultKind::MisalignedFpuVectorAddress { offset });
                }
                let mut value = [0; 4];
                for (lane, slot) in value.iter_mut().enumerate() {
                    *slot = self.read_data(self.data_address(offset.wrapping_add(lane as Word)))?
                        as i16;
                }
                self.fpu_registers[a] = value;
            }
            3 => {
                let offset = self.registers[b];
                if offset & 3 != 0 {
                    return Err(FaultKind::MisalignedFpuVectorAddress { offset });
                }
                let value = self.fpu_registers[a];
                for (lane, word) in value.into_iter().enumerate() {
                    self.write_data(
                        self.data_address(offset.wrapping_add(lane as Word)),
                        word as Word,
                    )?;
                }
            }
            4 => self.fpu_registers[a] = self.fpu_registers[b],
            5 => {
                if b > 12 {
                    return Err(FaultKind::InvalidInstruction);
                }
                let source = self.fpu_registers;
                self.fpu_registers[a] = std::array::from_fn(|lane| source[b + lane][0]);
            }
            6 => {
                if a > 12 {
                    return Err(FaultKind::InvalidInstruction);
                }
                let source = self.fpu_registers[b];
                for (lane, value) in source.into_iter().enumerate() {
                    self.fpu_registers[a + lane] = [value, 0, 0, 0];
                }
            }
            7 => {
                if a > 12 || b != 0 {
                    return Err(FaultKind::InvalidInstruction);
                }
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
            8..=10 => {
                let left = self.fpu_registers[a];
                let right = self.fpu_registers[b];
                self.fpu_registers[a] = std::array::from_fn(|lane| match function {
                    8 => fix16_add(left[lane], right[lane]),
                    9 => fix16_sub(left[lane], right[lane]),
                    10 => fix16_mul(left[lane], right[lane]),
                    _ => unreachable!(),
                });
            }
            11 => {
                let left = self.fpu_registers[a];
                let right = self.fpu_registers[b];
                for lane in 0..4 {
                    self.fpu_accumulator = acc_saturate(
                        i128::from(self.fpu_accumulator)
                            + i128::from(left[lane]) * i128::from(right[lane]),
                    );
                }
            }
            12 => {
                // FACCSTORE: `b` is a 4-bit destination write mask. Every set
                // bit writes the same rounded ACC value; ACC is then cleared.
                let value = fix16_from_acc(self.fpu_accumulator);
                for (lane, word) in self.fpu_registers[a].iter_mut().enumerate() {
                    if b >> lane & 1 == 1 {
                        *word = value;
                    }
                }
                self.fpu_accumulator = 0;
            }
            13 => {
                self.pending_test = Some(fix16_compare(
                    self.fpu_registers[a][0],
                    self.fpu_registers[b][0],
                ));
            }
            14 => self.execute_fpu_unary(a, b)?,
            // fn 15 is reserved (formerly FMULS).
            _ => return Err(FaultKind::InvalidInstruction),
        }
        Ok(StepOutcome::Running)
    }

    fn execute_fpu_unary(&mut self, register: usize, operation: usize) -> Result<(), FaultKind> {
        let source = self.fpu_registers[register];
        match operation {
            value if value == FpuUnaryOp::Reciprocal as usize => {
                self.fpu_registers[register][0] =
                    fix16_reciprocal(source[0]).map_err(FaultKind::FpuDomain)?;
            }
            value if value == FpuUnaryOp::ReciprocalSqrt as usize => {
                self.fpu_registers[register][0] =
                    fix16_reciprocal_sqrt(source[0]).map_err(FaultKind::FpuDomain)?;
            }
            value if value == FpuUnaryOp::SinCos as usize => {
                let (sin, cos) = fix16_sin_cos(source[0]);
                self.fpu_registers[register] = [sin, cos, 0, 0];
            }
            value if value == FpuUnaryOp::Abs as usize => {
                self.fpu_registers[register] = source.map(fix16_abs)
            }
            value if value == FpuUnaryOp::Neg as usize => {
                self.fpu_registers[register] = source.map(fix16_neg)
            }
            value if value == FpuUnaryOp::Floor as usize => {
                self.fpu_registers[register] = source.map(fix16_floor)
            }
            value if value == FpuUnaryOp::Ceil as usize => {
                self.fpu_registers[register] = source.map(fix16_ceil)
            }
            value if value == FpuUnaryOp::Round as usize => {
                self.fpu_registers[register] = source.map(fix16_round)
            }
            value if value == FpuUnaryOp::Saturate01 as usize => {
                self.fpu_registers[register] = source.map(fix16_saturate01)
            }
            value if value == FpuUnaryOp::Sign as usize => {
                self.fpu_registers[register] = source.map(fix16_sign)
            }
            value if value == FpuUnaryOp::Zero as usize => self.fpu_registers[register] = [0; 4],
            // FACCLOAD.X/Y/Z/W: overwrite ACC with the exact selected-lane
            // value in accumulator format (Q8.8 shifted left by 8).
            value if (FpuUnaryOp::AccLoadX as usize..=FpuUnaryOp::AccLoadW as usize)
                .contains(&value) =>
            {
                let lane = value - FpuUnaryOp::AccLoadX as usize;
                self.fpu_accumulator = i64::from(source[lane]) << 8;
            }
            _ => return Err(FaultKind::InvalidInstruction),
        }
        Ok(())
    }

    fn execute_control(&mut self, instruction: Word) -> ExecuteResult {
        let function = field(instruction, 8);
        let dst = field(instruction, 4);
        let src = field(instruction, 0);
        match function {
            0 => {
                self.registers[usize::from(dst)] =
                    self.registers[usize::from(src)].count_ones() as Word
            }
            1 => self.registers[usize::from(dst)] = self.registers[usize::from(src)],
            2 => self.registers[usize::from(dst)] = !self.registers[usize::from(src)],
            3 => self.registers[usize::from(dst)] = self.registers[usize::from(src)].wrapping_neg(),
            4 if dst == 0 => self.pc = self.registers[usize::from(src)],
            5 if dst == LINK_REGISTER => {
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
                    self.registers[usize::from(src)].leading_zeros() as Word
            }
            9 => {
                self.registers[usize::from(dst)] = Word::from(
                    (self.registers[usize::from(dst)] as i16)
                        < (self.registers[usize::from(src)] as i16),
                )
            }
            10 => {
                self.registers[usize::from(dst)] =
                    Word::from(self.registers[usize::from(dst)] < self.registers[usize::from(src)])
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
            8 if dst == 0 && src == 0 => {
                self.halted = true;
                return Ok(StepOutcome::Halted {
                    signal: self.registers[0],
                });
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
        |prefix| (prefix.high << 4) | (instruction & 15),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        alu, branch, compare_signed, compare_unsigned, device_receive, device_send, halt,
        immediate_high12, immediate_signed, immediate_unsigned, jump_and_link_register,
        jump_and_link_relative, jump_relative, jump_segment, load, load_immediate16, nop,
        population_count, prefixed, prefixed_branch, read_special, set_less_than_signed,
        set_less_than_unsigned, store, write_data_segment, AluOp, ImmediateOp, SpecialRegister,
        TestCondition,
    };

    #[test]
    fn executes_loop_and_unified_memory_round_trip() {
        let mut program = vec![];
        program.extend(load_immediate16(0, 0));
        program.extend(load_immediate16(1, 5));
        program.extend(load_immediate16(2, 0x4000));
        program.extend([
            alu(AluOp::Add, 0, 0, 1),
            immediate_signed(ImmediateOp::Add, 1, -1),
            immediate_signed(ImmediateOp::CompareSigned, 1, 0),
            branch(TestCondition::NotEqual, -4),
            store(0, 2, 0),
            load(3, 2, 0),
            halt(),
        ]);
        let mut machine = Machine::default();
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
        let mut machine = Machine::default();
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
        let mut machine = Machine::default();
        machine
            .load_program(0, &[0xfabc, 0xe111, 0xaf3d, halt()])
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
    fn fpu_domain_fault_is_precise() {
        let mut machine = Machine::default();
        machine
            .load_program(
                0,
                &[
                    crate::fpu(crate::FpuOp::Load, 0, 0),
                    crate::fpu_unary(0, crate::FpuUnaryOp::Reciprocal),
                ],
            )
            .unwrap();
        assert_eq!(machine.step(), Ok(StepOutcome::Running));
        assert_eq!(
            machine.step(),
            Err(Fault {
                kind: FaultKind::FpuDomain(FpuDomainError::ReciprocalZero),
                address: 1,
                instruction: crate::fpu_unary(0, crate::FpuUnaryOp::Reciprocal)
            })
        );
        assert_eq!(machine.fpu_register(0), Some([0; 4]));
    }

    #[test]
    fn fpu_vector_memory_dot_and_acc_writeback_follow_fix16_semantics() {
        let mut program = vec![];
        program.extend(load_immediate16(1, 0x0100));
        program.extend(load_immediate16(2, 0x0104));
        program.extend([
            crate::fpu(crate::FpuOp::Import4, 0, 1),
            crate::fpu(crate::FpuOp::Move, 1, 0),
            crate::fpu(crate::FpuOp::Dot4Acc, 0, 1),
            crate::fpu(crate::FpuOp::AccStore, 2, 1),
            crate::fpu(crate::FpuOp::Export4, 2, 2),
            halt(),
        ]);
        let mut machine = Machine::default();
        machine.load_program(0, &program).unwrap();
        machine
            .load_program(0x0100, &[256, 512, (-256_i16) as u16, 128])
            .unwrap();
        machine.run(32).unwrap();

        assert_eq!(machine.fpu_register(0), Some([256, 512, -256, 128]));
        assert_eq!(machine.fpu_register(2), Some([1600, 0, 0, 0]));
        assert_eq!(machine.fpu_accumulator(), 0);
        assert_eq!(machine.memory(0x0104), 1600);
        assert_eq!(machine.memory(0x0105), 0);
    }

    #[test]
    fn fpu_accstore_mask_and_accload_round_trip() {
        let mut program = vec![];
        program.extend(load_immediate16(1, 0x0100));
        program.extend([
            crate::fpu(crate::FpuOp::Import4, 0, 1),
            crate::fpu(crate::FpuOp::Move, 1, 0),
            crate::fpu(crate::FpuOp::Dot4Acc, 0, 1),
            // Mask 0b0101 writes lanes x and z; mask 0 only clears ACC.
            crate::fpu(crate::FpuOp::AccStore, 2, 0b0101),
            crate::fpu(crate::FpuOp::AccStore, 3, 0),
            // FACCLOAD.W overwrites ACC with lane w exactly; a full mask
            // splats it to every lane.
            crate::fpu_unary(0, crate::FpuUnaryOp::AccLoadW),
            crate::fpu(crate::FpuOp::AccStore, 4, 0b1111),
            halt(),
        ]);
        let mut machine = Machine::default();
        machine.load_program(0, &program).unwrap();
        machine
            .load_program(0x0100, &[256, 512, 768, 1024])
            .unwrap();
        machine.run(32).unwrap();
        // dot = 1 + 4 + 9 + 16 = 30.0 -> 7680.
        assert_eq!(machine.fpu_register(2), Some([7680, 0, 7680, 0]));
        assert_eq!(machine.fpu_register(3), Some([0; 4]));
        assert_eq!(machine.fpu_register(4), Some([1024; 4]));
        assert_eq!(machine.fpu_accumulator(), 0);
    }

    #[test]
    fn fpu_fn_15_is_reserved_and_faults() {
        let mut machine = Machine::default();
        machine.load_program(0, &[0xdf00]).unwrap();
        assert_eq!(
            machine.step(),
            Err(Fault {
                kind: FaultKind::InvalidInstruction,
                address: 0,
                instruction: 0xdf00
            })
        );
    }

    #[test]
    fn fpu_compare_sets_the_existing_pending_test() {
        let mut program = vec![];
        program.extend(load_immediate16(0, (-256_i16) as u16));
        program.extend(load_immediate16(1, 256));
        program.extend([
            crate::fpu(crate::FpuOp::Load, 0, 0),
            crate::fpu(crate::FpuOp::Load, 1, 1),
            crate::fpu(crate::FpuOp::Compare, 0, 1),
            branch(TestCondition::LessThan, 1),
            halt(),
            crate::move_register(0, 1),
            halt(),
        ]);
        let mut machine = Machine::default();
        machine.load_program(0, &program).unwrap();
        assert_eq!(
            machine.run(32).unwrap(),
            RunOutcome::Halted {
                steps: 10,
                signal: 256
            }
        );
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
        let mut machine = Machine::default();
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

        let mut machine = Machine::default();
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

        let mut machine = Machine::with_physical_memory_words(1 << 16);
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

        let mut machine = Machine::default();
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
            let mut machine = Machine::default();
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
        let mut machine = Machine::default();
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
        assert_eq!(program[1], immediate_high12(0x01));
        assert_eq!(program[2], 0xb003);
        let filler = immediate_unsigned(ImmediateOp::LoadUnsigned, 1, 1);
        program.extend(std::iter::repeat_n(filler, 0x103));
        program.extend([immediate_unsigned(ImmediateOp::LoadUnsigned, 1, 2), halt()]);

        let mut machine = Machine::default();
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
        let mut machine = Machine::default();
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
        let mut machine = Machine::default();
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
    fn reserved_branch_conditions_fault() {
        for condition in [0x6, 0x7, 0xa, 0xf] {
            let mut machine = Machine::default();
            machine
                .load_program(0, &[0xb000 | (condition << 8)])
                .unwrap();
            assert_eq!(
                machine.step(),
                Err(Fault {
                    kind: FaultKind::InvalidInstruction,
                    address: 0,
                    instruction: 0xb000 | (condition << 8),
                }),
                "condition {condition:#x}"
            );
        }
    }

    #[test]
    fn prefixes_are_transparent_to_the_pending_test() {
        // CMP; IMMHI12; BR: the prefix sits between producer and consumer.
        let mut machine = Machine::default();
        machine
            .load_program(
                0,
                &[
                    immediate_signed(ImmediateOp::CompareSigned, 0, 0),
                    immediate_high12(0),
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
        let mut machine = Machine::default();
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
        let mut machine = Machine::default();
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
        let mut machine = Machine::default();
        machine.load_program(0, &[0xe5d1]).unwrap();
        assert_eq!(
            machine.step(),
            Err(Fault {
                kind: FaultKind::InvalidInstruction,
                address: 0,
                instruction: 0xe5d1,
            })
        );

        let mut machine = Machine::default();
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

        let mut machine = Machine::default();
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
        let mut machine = Machine::default();
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
        let mut machine = Machine::default();
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
