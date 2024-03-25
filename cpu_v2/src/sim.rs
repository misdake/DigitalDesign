use crate::isa::{Flag4, Instruction};
use digital_design_code::select;

pub struct SimEnv {
    inst: Box<[Instruction; 65536]>,
    state: SimState,
}

struct SimState {
    reg: [u16; 16],
    mem: Box<[u16; 65536]>,
    pc: u16,
    sp: u16,
    flags: u8,
}

const FLAGS_GREATER: u8 = 1 << 0;
const FLAGS_EQUAL: u8 = 1 << 1;
const FLAGS_LESS: u8 = 1 << 2;
#[rustfmt::skip]
pub fn calc_flags(x: u16, y: u16) -> u8 {
    let mut r = 0;
    if x > y { r |= FLAGS_GREATER; }
    if x == y { r |= FLAGS_EQUAL; }
    if x < y { r |= FLAGS_LESS; }
    r
}

#[repr(u8)]
#[derive(Copy, Clone)]
pub enum Condition {
    Never = 0,
    Greater = FLAGS_GREATER,
    Equal = FLAGS_EQUAL,
    Less = FLAGS_LESS,
    NotEqual = FLAGS_GREATER | FLAGS_LESS,
    LessEqual = FLAGS_LESS | FLAGS_EQUAL,
    GreaterEqual = FLAGS_GREATER | FLAGS_EQUAL,
    Always = FLAGS_GREATER | FLAGS_EQUAL | FLAGS_LESS,

    Call = 15,
}
impl Condition {
    /// Return (jmp_enable, is_call)
    fn check_flags(self, flags: u8) -> (bool, bool) {
        let c = self as u8;
        let is_call = c == Condition::Call as u8;
        let cond = c & 0b111;
        let jmp_enable = is_call || (cond & flags > 0);
        (jmp_enable, is_call)
    }
}

impl Default for SimState {
    fn default() -> Self {
        Self {
            reg: [0; 16],
            mem: Box::new([0; 65536]),
            pc: 0,
            sp: 0,
            flags: 0,
        }
    }
}

#[derive(Default, Copy, Clone, Debug, Eq, PartialEq)]
pub struct StateChange {
    reg: Option<(u8, u16)>,  // addr, data
    mem: Option<(u16, u16)>, // addr, data
    pc_next: Option<u16>,    // only Some when it's not pc+1
    sp: Option<u16>,
    flags: Option<u8>,
}
impl StateChange {
    fn reg(&mut self, r: u8, data: u16) {
        assert!(self.reg.is_none());
        self.reg = Some((r, data));
    }
    fn mem(&mut self, addr: u16, data: u16) {
        assert!(self.mem.is_none());
        self.mem = Some((addr, data));
    }
    fn pc_next(&mut self, pc_next: u16) {
        assert!(self.pc_next.is_none());
        self.pc_next = Some(pc_next);
    }
    fn sp(&mut self, sp: u16) {
        assert!(self.sp.is_none());
        self.sp = Some(sp);
    }
    fn flags(&mut self, flags: u8) {
        assert!(self.flags.is_none());
        self.flags = Some(flags);
    }
}

impl SimEnv {
    pub fn new(inst: Box<[Instruction; 65536]>) -> SimEnv {
        Self {
            inst,
            state: SimState::default(),
        }
    }

    pub fn eval(&self) -> StateChange {
        let mut changes = StateChange::default();

        let pc = self.state.pc;
        let inst = self.inst[pc as usize];
        let reg = |r: u8| self.state.reg[r as usize];
        let mem = |addr: u16| self.state.mem[addr as usize];
        let sp = self.state.sp;

        fn j(state: &SimState, cond: Flag4, changes: &mut StateChange, f: impl FnOnce(u16) -> u16) {
            let cond: Condition = unsafe { std::mem::transmute(cond) };
            let (jmp, is_call) = cond.check_flags(state.flags);
            if jmp {
                changes.pc_next(f(state.pc));
            }
            if is_call {
                changes.sp(state.sp - 1);
                changes.mem(state.sp - 1, state.pc + 1);
            }
        }

        // real sim
        match inst {
            Instruction::and(r2, r1, r0) => changes.reg(r0, reg(r1) & reg(r2)),
            Instruction::or(r2, r1, r0) => changes.reg(r0, reg(r1) | reg(r2)),
            Instruction::xor(r2, r1, r0) => changes.reg(r0, reg(r1) ^ reg(r2)),
            Instruction::add(r2, r1, r0) => changes.reg(r0, reg(r1).wrapping_add(reg(r2))),
            Instruction::sub(r2, r1, r0) => changes.reg(r0, reg(r1).wrapping_sub(reg(r2))),
            Instruction::lsl(imm, r1, r0) => changes.reg(r0, reg(r1) << imm),
            Instruction::lsr(imm, r1, r0) => changes.reg(r0, reg(r1) >> imm),

            Instruction::mov(r1, r0) => changes.reg(r0, reg(r1)),
            Instruction::inv(r1, r0) => changes.reg(r0, !reg(r1)),
            Instruction::neg(r1, r0) => changes.reg(r0, u16::MAX - reg(r1)),
            Instruction::addi(imm, r1, r0) => changes.reg(r0, reg(r1).wrapping_sub(reg(imm))),
            Instruction::cnt1(r1, r0) => changes.reg(r0, reg(r1).count_ones() as u16),
            Instruction::log2(r1, r0) => changes.reg(r0, reg(r1).ilog2() as u16),
            Instruction::not0(r1, r0) => changes.reg(r0, select(reg(r1) != 0, 1, 0)),
            Instruction::cmp_i(u4, r0) => changes.flags(calc_flags(reg(r0), u4 as u16)),
            Instruction::cmp_r(r1, r0) => changes.flags(calc_flags(reg(r0), reg(r1))),

            Instruction::load_hi(hi, lo, r0) => changes.reg(
                r0,
                (((hi as u16) << 12) | ((lo as u16) << 8)) | (reg(r0) & 0b11111111),
            ),
            Instruction::load_lo(hi, lo, r0) => changes.reg(r0, ((hi as u16) << 4) | (lo as u16)),

            Instruction::store_mem(r1, r0) => changes.mem(reg(r1), reg(r0)),
            Instruction::load_mem(r1, r0) => changes.reg(r0, mem(reg(r1))),
            Instruction::stack_write(u4, r0) => changes.mem(sp + u4 as u16, reg(r0)),
            Instruction::stack_read(u4, r0) => changes.reg(r0, mem(sp + u4 as u16)),
            Instruction::stack_push(r0) => {
                changes.sp(sp - 1);
                changes.mem(sp - 1, reg(r0));
            }
            Instruction::stack_pop(r0) => {
                changes.reg(r0, mem(sp));
                changes.sp(sp + 1);
            }
            Instruction::stack_push_pc() => {
                changes.sp(sp - 1);
                changes.mem(sp - 1, pc + 1);
            }
            Instruction::stack_pop_pc() => {
                changes.pc_next(mem(sp));
                changes.sp(sp + 1);
            }

            Instruction::sp_set_r(u4, r0) => changes.sp(reg(r0) + u4 as u16),
            Instruction::sp_add_r(r0) => changes.sp(sp + reg(r0)),
            Instruction::sp_inc_i(u4) => changes.sp(sp.wrapping_add(u4 as u16)),
            Instruction::sp_dec_i(u4) => changes.sp(sp.wrapping_sub(u4 as u16)),
            Instruction::sp_get_i(u4, r0) => changes.reg(r0, sp.wrapping_add(u4 as u16)),
            Instruction::sp_get_r(r1, r0) => changes.reg(r0, sp.wrapping_add(reg(r1))),

            Instruction::j_add_i(hi, lo, cond) => {
                j(&self.state, cond, &mut changes, |pc| {
                    pc.wrapping_add(((hi << 4) | lo) as u16)
                });
            }
            Instruction::j_add_r(r1, cond) => {
                j(&self.state, cond, &mut changes, |pc| {
                    pc.wrapping_add(reg(r1))
                });
            }
            Instruction::j_set_r(r1, cond) => {
                j(&self.state, cond, &mut changes, |_pc| reg(r1));
            }

            Instruction::dev_recv(_idx, _op, _r0) => todo!(),
            Instruction::dev_send(_idx, _op, _r0) => todo!(),
        }

        changes
    }
    pub fn test(&self, ref_changes: StateChange) -> SimTestResult {
        //TODO call eval
        //TODO new result
        todo!()
    }

    pub fn commit(&mut self, changes: StateChange) {
        if let Some((r, data)) = changes.reg {
            self.state.reg[r as usize] = data;
        }
        if let Some((m, data)) = changes.mem {
            self.state.mem[m as usize] = data;
        }
        if let Some(pc_next) = changes.pc_next {
            self.state.pc = pc_next;
        } else {
            self.state.pc += 1;
        }
        if let Some(sp) = changes.sp {
            self.state.sp = sp;
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
