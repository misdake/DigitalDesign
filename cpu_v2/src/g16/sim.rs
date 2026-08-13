//! Small architectural interpreter used as the G16 correctness oracle.

use super::encoding::{is_prefix_consumer, sign_extend, Word};

pub const MEMORY_WORDS: usize = 1 << 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FaultKind {
    InvalidInstruction,
    UnsupportedFpu,
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

pub struct Machine {
    registers: [Word; 16],
    memory: Box<[Word; MEMORY_WORDS]>,
    pc: Word,
    prefix: Option<Prefix>,
    retired_words: u64,
    halted: bool,
}

impl Default for Machine {
    fn default() -> Self {
        Self {
            registers: [0; 16],
            memory: Box::new([0; MEMORY_WORDS]),
            pc: 0,
            prefix: None,
            retired_words: 0,
            halted: false,
        }
    }
}

impl Machine {
    pub fn load_program(&mut self, base: Word, words: &[Word]) -> Result<(), ProgramLoadError> {
        let start = usize::from(base);
        let end = start
            .checked_add(words.len())
            .filter(|end| *end <= MEMORY_WORDS)
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
        self.memory[usize::from(address)]
    }

    pub fn pc(&self) -> Word {
        self.pc
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
        let instruction = self.memory[usize::from(address)];
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
        self.registers[usize::from(dst)] = self.memory[usize::from(address)];
        Ok(StepOutcome::Running)
    }

    fn execute_store(&mut self, instruction: Word, prefix: Option<Prefix>) -> ExecuteResult {
        let src = field(instruction, 8);
        let base = self.registers[usize::from(field(instruction, 4))];
        let address = base.wrapping_add(immediate4(instruction, prefix, true));
        self.memory[usize::from(address)] = self.registers[usize::from(src)];
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
}

type ExecuteResult = Result<StepOutcome, FaultKind>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProgramLoadError {
    pub base: Word,
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
        alu, branch, halt, immediate_signed, load, load_immediate16, store, AluOp, BranchCondition,
        ImmediateOp,
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
}
