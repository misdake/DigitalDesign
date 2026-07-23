use crate::isa::*;
use digital_design_code::select;

pub struct SimEnv {
    pub inst: Box<[Instruction; 65536]>,
    pub state: SimState,
}

pub struct SimState {
    pub cycles: usize,
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

#[rustfmt::skip]
pub fn calc_flags_signed(x: u16, y: u16) -> u8 {
    let (x, y) = (x as i16, y as i16);
    let mut r = 0;
    if x > y { r |= FLAGS_GREATER; }
    if x == y { r |= FLAGS_EQUAL; }
    if x < y { r |= FLAGS_LESS; }
    r
}

impl Default for SimState {
    fn default() -> Self {
        Self {
            cycles: 0,
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
    pub halt: Option<u16>,
}
impl StateChange {
    fn new(pc_next: u16) -> Self {
        Self {
            pc_next,
            reg: None,
            mem: None,
            flags: None,
            halt: None,
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
    fn halt(&mut self, signal: u16) {
        self.halt = Some(signal);
    }

    pub fn desc(&self, pc: u16) -> String {
        let mut outputs = vec![];
        if pc + 1 != self.pc_next {
            outputs.push(format!("pc = {:04x}", self.pc_next));
        }
        if let Some((r, data)) = self.reg {
            outputs.push(format!("r{0} = {1} ({1:04x})", r, data));
        }
        if let Some((addr, data)) = self.mem {
            outputs.push(format!("mem[{0:04x}] = {1} ({1:04x})", addr, data));
        }
        if let Some(flags) = self.flags {
            let g = select((flags & FLAGS_GREATER) != 0, "g", "");
            let l = select((flags & FLAGS_LESS) != 0, "l", "");
            let e = select((flags & FLAGS_EQUAL) != 0, "e", "");
            outputs.push(format!("flags = {g}{l}{e}"));
        }
        if let Some(signal) = self.halt {
            outputs.push(format!("halt {0} ({0:04x})", signal));
        }
        outputs.join(", ")
    }
}

impl SimEnv {
    pub fn new(inst: &[Instruction]) -> SimEnv {
        let mut inst_array = Box::new([Instruction::halt(0); 65536]);
        assert!(inst.len() <= 65536);
        inst_array[..inst.len()].copy_from_slice(inst);

        Self {
            inst: inst_array,
            state: SimState::default(),
        }
    }

    pub fn run_to_halt(
        &mut self,
        max_cycle: usize,
        on_inst: impl Fn(u16, Instruction, &StateChange),
    ) -> Option<u16> {
        for _ in 0..max_cycle {
            let change = self.eval();
            on_inst(self.state.pc, self.inst[self.state.pc as usize], &change);
            self.commit(change);
            if let Some(signal) = change.halt {
                return Some(signal);
            }
        }
        None
    }

    pub fn eval(&self) -> StateChange {
        use Cond::*;

        let pc = self.state.pc;
        let inst = self.inst[pc as usize];
        let reg = |r: u8| self.state.reg[r as usize];
        let mem = |addr: u16| self.state.mem[addr as usize];
        let mut changes = StateChange::new(pc + 1);

        fn j_cc(state: &SimState, cond: Cond, changes: &mut StateChange, hi: u8, lo: u8) {
            let jmp = state.flags & (cond as u8) > 0;
            if jmp {
                let pc_next = state.pc.wrapping_add(imm8_as_i16(hi, lo));
                changes.pc_next(pc_next);
            }
        }

        match inst {
            Instruction::halt(r1) => {
                changes.halt(reg(r1));
            }
            Instruction::and(r2, r1, r0) => changes.reg(r0, reg(r2) & reg(r1)),
            Instruction::or(r2, r1, r0) => changes.reg(r0, reg(r2) | reg(r1)),
            Instruction::xor(r2, r1, r0) => changes.reg(r0, reg(r2) ^ reg(r1)),
            Instruction::add(r2, r1, r0) => changes.reg(r0, reg(r2).wrapping_add(reg(r1))),
            Instruction::sub(r2, r1, r0) => changes.reg(r0, reg(r2).wrapping_sub(reg(r1))),
            Instruction::addi(r2, i4, r0) => {
                changes.reg(r0, reg(r2).wrapping_add(imm4_nz(i4) as u16))
            }
            Instruction::lsl(u4, r0) => changes.reg(r0, reg(r0) << u4),
            Instruction::lsr(u4, r0) => changes.reg(r0, reg(r0) >> u4),
            Instruction::asr(u4, r0) => changes.reg(r0, ((reg(r0) as i16) >> u4) as u16),

            Instruction::mov(r1, r0) => changes.reg(r0, reg(r1)),
            Instruction::inv(r1, r0) => changes.reg(r0, !reg(r1)),
            Instruction::neg(r1, r0) => changes.reg(r0, -(reg(r1) as i16) as u16),
            Instruction::cnt1(r1, r0) => changes.reg(r0, reg(r1).count_ones() as u16),
            Instruction::log2(r1, r0) => changes.reg(r0, reg(r1).ilog2() as u16),
            Instruction::not0(r1, r0) => changes.reg(r0, select(reg(r1) != 0, 1, 0)),
            Instruction::sp_add(hi, lo) => {
                changes.reg(SP_REG, reg(SP_REG).wrapping_add(hilo_as_u16(hi, lo)))
            }
            Instruction::sp_sub(hi, lo) => {
                changes.reg(SP_REG, reg(SP_REG).wrapping_sub(hilo_as_u16(hi, lo)))
            }
            Instruction::pc(i4, r0) => changes.reg(r0, pc.wrapping_add(imm_as_i16(i4))),

            Instruction::load_hi(hi, lo, r0) => changes.reg(
                r0,
                (((hi as u16) << 12) | ((lo as u16) << 8)) | (reg(r0) & 0b11111111),
            ),
            Instruction::load_lo(hi, lo, r0) => changes.reg(r0, ((hi as u16) << 4) | (lo as u16)),

            Instruction::store_mem(r2, r1, offset) => {
                let addr = reg(r2).wrapping_add(imm_as_i16(offset));
                changes.mem(addr, reg(r1))
            }
            Instruction::load_mem(r2, offset, r0) => {
                let addr = reg(r2).wrapping_add(imm_as_i16(offset));
                changes.reg(r0, mem(addr))
            }
            Instruction::store_sp(hi, lo, r0) => {
                let addr = reg(SP_REG).wrapping_add(hilo_as_u16(hi, lo));
                changes.mem(addr, reg(r0))
            }
            Instruction::load_sp(hi, lo, r0) => {
                let addr = reg(SP_REG).wrapping_add(hilo_as_u16(hi, lo));
                changes.reg(r0, mem(addr))
            }

            Instruction::jg(hi, lo) => j_cc(&self.state, Greater, &mut changes, hi, lo),
            Instruction::je(hi, lo) => j_cc(&self.state, Equal, &mut changes, hi, lo),
            Instruction::jge(hi, lo) => j_cc(&self.state, GreaterEqual, &mut changes, hi, lo),
            Instruction::jl(hi, lo) => j_cc(&self.state, Less, &mut changes, hi, lo),
            Instruction::jne(hi, lo) => j_cc(&self.state, NotEqual, &mut changes, hi, lo),
            Instruction::jle(hi, lo) => j_cc(&self.state, LessEqual, &mut changes, hi, lo),
            Instruction::jmp(hi, lo) => j_cc(&self.state, Always, &mut changes, hi, lo),
            Instruction::cmp_r(r1, r0) => changes.flags(calc_flags(reg(r0), reg(r1))),
            Instruction::cmp_i(u4, r0) => changes.flags(calc_flags(reg(r0), u4 as u16)),
            Instruction::cmp_s(r1, r0) => changes.flags(calc_flags_signed(reg(r0), reg(r1))),
            Instruction::cmp_si(i4, r0) => {
                changes.flags(calc_flags_signed(reg(r0), imm_as_i16(i4)))
            }
            Instruction::call_rel(hi, lo) => {
                changes.reg(RA_REG, pc + 1);
                changes.pc_next(pc.wrapping_add(imm8_as_i16(hi, lo)));
            }
            Instruction::call_abs(hi, lo) => {
                changes.reg(RA_REG, pc + 1);
                changes.pc_next(mem(0xff00 + hilo_as_u16(hi, lo)));
            }
            Instruction::jmp_reg(r1) => changes.pc_next(reg(r1)),
            Instruction::call_reg(r1) => {
                changes.reg(RA_REG, pc + 1);
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
        self.state.cycles += 1;
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
