use crate::isa::*;
use digital_design_code::select;
use std::ops::Shr;

pub struct SimEnv {
    pub inst: Box<[Instruction; 65536]>,
    pub state: SimState,
}

pub struct SimState {
    pub reg: [u16; 16],
    pub mem: Box<[u16; 65536]>,
    pub pc: u16,
    pub flags: u8,
}

#[rustfmt::skip]
pub fn calc_flags(x: u16, y: u16) -> u8 {
    let mut r = 0;
    if x > y { r |= FLAGS_GREATER; }
    if x == y { r |= FLAGS_EQUAL; }
    if x < y { r |= FLAGS_LESS; }
    r
}

impl Default for SimState {
    fn default() -> Self {
        Self {
            reg: [0; 16],
            mem: Box::new([0; 65536]),
            pc: 0,
            flags: 0,
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct StateChange {
    pub pc_next: u16,
    pub reg: Option<(u8, u16)>,  // addr, data
    pub mem: Option<(u16, u16)>, // addr, data
    pub flags: Option<u8>,
    pub halt: bool,
}
impl StateChange {
    fn new(pc_next: u16) -> Self {
        Self {
            pc_next,
            reg: None,
            mem: None,
            flags: None,
            halt: false,
        }
    }
    fn reg(&mut self, r: u8, data: u16) {
        assert!(self.reg.is_none());
        self.reg = Some((r, data));
    }
    fn mem(&mut self, addr: u16, data: u16) {
        assert!(self.mem.is_none());
        self.mem = Some((addr, data));
    }
    fn pc_next(&mut self, pc_next: u16) {
        self.pc_next = pc_next;
    }
    fn flags(&mut self, flags: u8) {
        assert!(self.flags.is_none());
        self.flags = Some(flags);
    }
    fn halt(&mut self) {
        self.halt = true;
    }
}

impl SimEnv {
    pub fn new(inst: &[Instruction]) -> SimEnv {
        let mut inst_array = box [Instruction::halt(); 65536];
        assert!(inst.len() <= 65536);
        inst_array[..inst.len()].copy_from_slice(inst);

        Self {
            inst: inst_array,
            state: SimState::default(),
        }
    }

    pub fn run_to_halt(&mut self, max_cycle: usize) -> usize {
        for i in 0..max_cycle {
            let change = self.eval();
            if change.halt {
                return i;
            }
            self.commit(change);
        }
        max_cycle
    }

    pub fn eval(&self) -> StateChange {
        use Cond::*;

        let pc = self.state.pc;
        let inst = self.inst[pc as usize];
        let reg = |r: u8| self.state.reg[r as usize];
        let mem = |addr: u16| self.state.mem[addr as usize];
        let mut changes = StateChange::new(pc + 1);

        fn j_offset(state: &SimState, cond: Cond, changes: &mut StateChange, offset: u16) {
            let jmp = state.flags & (cond as u8) > 0;
            if jmp {
                changes.pc_next(state.pc.wrapping_add(offset));
            }
        }

        match inst {
            Instruction::halt() => {
                changes.halt();
            }
            Instruction::and(r2, r1, r0) => changes.reg(r0, reg(r1) & reg(r2)),
            Instruction::or(r2, r1, r0) => changes.reg(r0, reg(r1) | reg(r2)),
            Instruction::xor(r2, r1, r0) => changes.reg(r0, reg(r1) ^ reg(r2)),
            Instruction::add(r2, r1, r0) => changes.reg(r0, reg(r1).wrapping_add(reg(r2))),
            Instruction::sub(r2, r1, r0) => changes.reg(r0, reg(r1).wrapping_sub(reg(r2))),
            Instruction::addi(r2, u4, r0) => changes.reg(r0, reg(r2).wrapping_add(u4 as u16)),
            Instruction::subi(r2, u4, r0) => changes.reg(r0, reg(r2).wrapping_sub(u4 as u16)),
            Instruction::lsl(u4, r0) => changes.reg(r0, reg(r0) << u4),
            Instruction::lsr(u4, r0) => changes.reg(r0, reg(r0) >> u4),
            Instruction::asr(u4, r0) => changes.reg(r0, ((reg(r0) as i16) >> u4) as u16),

            Instruction::mov(r1, r0) => changes.reg(r0, reg(r1)),
            Instruction::inv(r1, r0) => changes.reg(r0, !reg(r1)),
            Instruction::neg(r1, r0) => changes.reg(r0, u16::MAX - reg(r1)),
            Instruction::cnt1(r1, r0) => changes.reg(r0, reg(r1).count_ones() as u16),
            Instruction::log2(r1, r0) => changes.reg(r0, reg(r1).ilog2() as u16),
            Instruction::not0(r1, r0) => changes.reg(r0, select(reg(r1) != 0, 1, 0)),
            Instruction::cmp_i(u4, r0) => changes.flags(calc_flags(reg(r0), u4 as u16)),
            Instruction::pc(i4, r0) => changes.reg(r0, pc.wrapping_add(imm_as_i16(i4))),
            Instruction::cmp_r(r1, r0) => changes.flags(calc_flags(reg(r0), reg(r1))),

            Instruction::load_hi(hi, lo, r0) => changes.reg(
                r0,
                (((hi as u16) << 12) | ((lo as u16) << 8)) | (reg(r0) & 0b11111111),
            ),
            Instruction::load_lo(hi, lo, r0) => changes.reg(r0, ((hi as u16) << 4) | (lo as u16)),

            Instruction::store_mem(r2, offset, r0) => {
                let addr = reg(r2).wrapping_add(imm_as_i16(offset));
                changes.mem(addr, reg(r0))
            }
            Instruction::load_mem(r2, offset, r0) => {
                let addr = reg(r2).wrapping_add(imm_as_i16(offset));
                changes.reg(r0, mem(addr))
            }

            Instruction::j_offset_g(lo, hi) => {
                j_offset(&self.state, Greater, &mut changes, hilo_as_u16(hi, lo));
            }
            Instruction::j_offset_e(lo, hi) => {
                j_offset(&self.state, Equal, &mut changes, hilo_as_u16(hi, lo));
            }
            Instruction::j_offset_l(lo, hi) => {
                j_offset(&self.state, Less, &mut changes, hilo_as_u16(hi, lo));
            }
            Instruction::j_offset(lo, hi) => {
                j_offset(&self.state, Always, &mut changes, hilo_as_u16(hi, lo));
            }
            Instruction::j_offset_le(lo, hi) => {
                j_offset(&self.state, LessEqual, &mut changes, hilo_as_u16(hi, lo));
            }
            Instruction::j_offset_ne(lo, hi) => {
                j_offset(&self.state, NotEqual, &mut changes, hilo_as_u16(hi, lo));
            }
            Instruction::j_offset_ge(lo, hi) => {
                j_offset(&self.state, GreaterEqual, &mut changes, hilo_as_u16(hi, lo));
            }
            Instruction::jmp_reg(r1) => changes.pc_next(reg(r1)),
            Instruction::call_reg(r1, r0) => {
                changes.reg(r0, pc + 1);
                changes.pc_next(reg(r1));
            }

            Instruction::dev_recv(_idx, _op, _r0) => todo!(),
            Instruction::dev_send(_idx, _op, _r0) => todo!(),
        }

        changes
    }

    pub fn test(&self, real_changes: StateChange) -> SimTestResult {
        let sim_changes = self.eval();
        SimTestResult::new(sim_changes, real_changes)
    }

    pub fn commit(&mut self, changes: StateChange) {
        self.state.pc = changes.pc_next;
        if let Some((r, data)) = changes.reg {
            self.state.reg[r as usize] = data;
        }
        if let Some((m, data)) = changes.mem {
            self.state.mem[m as usize] = data;
        }
        if let Some(flags) = changes.flags {
            self.state.flags = flags;
        }
    }
}

pub struct SimTestResult {
    pub pass: bool,
    pub sim_changes: StateChange,
    pub ref_changes: StateChange,
}
impl SimTestResult {
    pub fn new(sim_changes: StateChange, ref_changes: StateChange) -> Self {
        Self {
            pass: sim_changes == ref_changes,
            sim_changes,
            ref_changes,
        }
    }
    //TODO to string? debug?
    //TODO is_passed()
}

fn imm_as_i16(i4: Imm4) -> u16 {
    let sign_bit = (i4 & 0b1000) != 0;
    i4 as u16 | (0b1111_1111_1111_0000 * sign_bit as u16)
}
fn hilo_as_u16(hi: Imm4, lo: Imm4) -> u16 {
    ((hi as u16) << 4) | (lo as u16)
}
