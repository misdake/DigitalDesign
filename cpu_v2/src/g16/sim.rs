//! Small architectural interpreter used as the G16 correctness oracle.

use super::encoding::{is_prefix_consumer, sign_extend, SpecialRegister, Word};
use super::{PhysicalWordAddress, MMIO_BASE};

/// Fitted 8-MiB SDRAM on the initial Tang Nano 20K target.
pub const DEFAULT_PHYSICAL_MEMORY_WORDS: usize = 1 << 22;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FaultKind {
    InvalidInstruction,
    UnsupportedFpu,
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

/// A device attached to the fixed MMIO page. Device `d` owns the sixteen
/// words at `MMIO_BASE + d * 16`; loads and stores to those words are routed
/// to the device instead of memory. `memory` is the complete physical memory,
/// so DMA-style devices can move data.
pub trait Device {
    fn read(&mut self, memory: &mut [Word], channel: u8) -> Word;
    fn write(&mut self, memory: &mut [Word], channel: u8, value: Word);
    /// Downcast support for test assertions on attached models.
    fn as_any(&self) -> &dyn std::any::Any;
}

/// Maps a physical address onto its MMIO `(device, channel)` pair, if any.
fn device_channel(address: PhysicalWordAddress) -> Option<(usize, u8)> {
    let raw = address.get();
    (u32::from(MMIO_BASE)..(u32::from(MMIO_BASE) + 256))
        .contains(&raw)
        .then(|| (((raw - u32::from(MMIO_BASE)) >> 4) as usize, (raw & 15) as u8))
}

pub struct Machine {
    registers: [Word; 16],
    memory: Box<[Word]>,
    devices: [Option<Box<dyn Device>>; 16],
    /// Optional BSRAM boot-window image: instruction fetches from the lowest
    /// physical words read this image instead of main memory, matching the
    /// hardware split between the boot BSRAM (instruction side) and SDRAM
    /// (data side). Without it, memory is fully unified.
    boot_window: Option<Box<[Word]>>,
    pc: Word,
    code_segment: Word,
    data_segment: Word,
    prefix: Option<Prefix>,
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
            "G16 physical memory must contain at least one word"
        );
        assert!(
            words <= (u32::MAX as usize) + 1,
            "G16 physical memory exceeds the architectural word address space"
        );
        Self {
            registers: [0; 16],
            memory: vec![0; words].into_boxed_slice(),
            devices: std::array::from_fn(|_| None),
            boot_window: None,
            pc: 0,
            code_segment: 0,
            data_segment: 0,
            prefix: None,
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

    /// Attaches a device to the fixed MMIO page; loads and stores to the
    /// device's sixteen words are routed to it instead of memory.
    pub fn attach_device(&mut self, device: u8, handler: Box<dyn Device>) {
        assert!(device < 16, "G16 device index {device} exceeds the MMIO page");
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
            0xb => self.execute_branch(instruction, prefix),
            0xc => self.execute_jump(instruction, prefix),
            0xd => Err(FaultKind::UnsupportedFpu),
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

    fn execute_branch(&mut self, instruction: Word, prefix: Option<Prefix>) -> ExecuteResult {
        let condition = field(instruction, 8);
        let value = self.registers[usize::from(field(instruction, 4))];
        let taken = match condition {
            0 => value == 0,
            1 => value != 0,
            2 => (value as i16) < 0,
            3 => (value as i16) >= 0,
            4 => (value as i16) > 0,
            5 => (value as i16) <= 0,
            6 => value & 1 != 0,
            7 => value & 1 == 0,
            _ => return Err(FaultKind::InvalidInstruction),
        };
        if taken {
            self.pc = self.pc.wrapping_add(immediate4(instruction, prefix, true));
        }
        Ok(StepOutcome::Running)
    }

    fn execute_jump(&mut self, instruction: Word, prefix: Option<Prefix>) -> ExecuteResult {
        let link = field(instruction, 8);
        let offset = prefix.map_or_else(
            || sign_extend(instruction & 0xff, 8),
            |prefix| ((prefix.high & 0xff) << 8) | (instruction & 0xff),
        );
        if link != 15 {
            self.registers[usize::from(link)] = self.pc;
        }
        self.pc = self.pc.wrapping_add(offset);
        Ok(StepOutcome::Running)
    }

    fn execute_control(&mut self, instruction: Word) -> ExecuteResult {
        let function = field(instruction, 8);
        let dst = field(instruction, 4);
        let src = field(instruction, 0);
        match function {
            1 => self.registers[usize::from(dst)] = self.registers[usize::from(src)],
            2 => self.registers[usize::from(dst)] = !self.registers[usize::from(src)],
            3 => self.registers[usize::from(dst)] = self.registers[usize::from(src)].wrapping_neg(),
            4 if dst == 0 => self.pc = self.registers[usize::from(src)],
            5 => {
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
                self.registers[usize::from(dst)] =
                    self.registers[usize::from(src)].count_ones() as Word
            }
            12 => {
                self.registers[usize::from(dst)] = match src {
                    value if value == SpecialRegister::CodeSegment as u8 => self.code_segment,
                    value if value == SpecialRegister::DataSegment as u8 => self.data_segment,
                    _ => return Err(FaultKind::InvalidInstruction),
                }
            }
            13 if dst == SpecialRegister::DataSegment as u8 => {
                self.data_segment = self.registers[usize::from(src)];
            }
            14 => {
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
        // The top offset page is a fixed system/MMIO window. Keeping it in
        // segment zero preserves the existing r15-based ABI when DSEG changes.
        let segment = if offset >= MMIO_BASE {
            0
        } else {
            self.data_segment
        };
        PhysicalWordAddress::from_segment_offset(segment, offset)
    }

    fn read_data(&mut self, address: PhysicalWordAddress) -> Result<Word, FaultKind> {
        if let Some((device, channel)) = device_channel(address) {
            if let Some(handler) = self.devices[device].as_mut() {
                return Ok(handler.read(&mut self.memory, channel));
            }
        }
        self.read_physical(address)
    }

    fn write_data(&mut self, address: PhysicalWordAddress, value: Word) -> Result<(), FaultKind> {
        if let Some((device, channel)) = device_channel(address) {
            if let Some(handler) = self.devices[device].as_mut() {
                handler.write(&mut self.memory, channel, value);
                return Ok(());
            }
        }
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
    use crate::g16::{
        alu, branch, halt, immediate_signed, jump_segment, load, load_immediate16,
        population_count, read_special, set_less_than_signed, set_less_than_unsigned, store,
        write_data_segment, AluOp, BranchCondition, ImmediateOp, SpecialRegister,
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
            branch(BranchCondition::NonZero, 1, -3),
            store(0, 2, 0),
            load(3, 2, 0),
            halt(),
        ]);
        let mut machine = Machine::default();
        machine.load_program(0, &program).unwrap();

        assert_eq!(
            machine.run(100).unwrap(),
            RunOutcome::Halted {
                steps: 24,
                signal: 15
            }
        );
        assert_eq!(machine.register(3), Some(15));
        assert_eq!(machine.memory(0x4000), 15);
        assert_eq!(machine.retired_words(), 24);
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
    fn fpu_opcode_has_a_distinct_fault() {
        let mut machine = Machine::default();
        machine.load_program(0, &[0xd000]).unwrap();
        assert_eq!(
            machine.step(),
            Err(Fault {
                kind: FaultKind::UnsupportedFpu,
                address: 0,
                instruction: 0xd000
            })
        );
    }

    #[test]
    fn revision_three_comparisons_cover_overflow_edges() {
        let mut program = vec![];
        program.extend(load_immediate16(1, 0x8000));
        program.extend(load_immediate16(2, 0x7fff));
        program.extend([
            crate::g16::move_register(3, 1),
            set_less_than_signed(3, 2),
            crate::g16::move_register(4, 1),
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
    fn mmio_page_does_not_move_with_the_data_segment() {
        let mut program = vec![];
        program.extend(load_immediate16(1, 3));
        program.extend(load_immediate16(2, MMIO_BASE));
        program.extend([write_data_segment(1), load(0, 2, 0), halt()]);

        let mut machine = Machine::default();
        machine.load_program(0, &program).unwrap();
        machine.load_program(MMIO_BASE, &[0x55aa]).unwrap();
        assert_eq!(
            machine.run(16).unwrap(),
            RunOutcome::Halted {
                steps: 7,
                signal: 0x55aa,
            }
        );
    }
}
