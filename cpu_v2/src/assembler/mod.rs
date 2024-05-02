#![allow(clippy::manual_range_contains)]

use crate::isa::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

#[derive(Copy, Clone)]
struct InstructionSlot {
    addr: usize,
}
struct PendingJump {
    inst: fn(&mut AssemblerInner, InstructionSlot, Cond) -> InstructionSlot,
    addr: InstructionSlot,
    cond: Cond,
}

pub struct AssemblerInner {
    instructions: Box<[Instruction; 65536]>,
    inst_valid: Box<[bool; 65536]>,
    functions: HashMap<&'static str, InstructionSlot>,
    comments: HashMap<usize, String>,

    cursor: usize,
}

/// core functions
impl AssemblerInner {
    fn new() -> Self {
        Self {
            instructions: box [halt(); 65536],
            inst_valid: box [false; 65536],
            functions: HashMap::new(),
            comments: HashMap::new(),
            cursor: 0,
        }
    }

    fn set_cursor(&mut self, addr: usize) {
        self.cursor = addr;
    }
    fn inst_at(&mut self, inst: Instruction, addr: usize) -> InstructionSlot {
        assert!(!self.inst_valid[addr]);
        self.inst_valid[addr] = true;
        self.instructions[addr] = inst;
        InstructionSlot { addr }
    }
    fn inst(&mut self, inst: Instruction) -> InstructionSlot {
        let addr = self.cursor;
        self.cursor += 1;
        self.inst_at(inst, addr)
    }
    fn comment(&mut self, slot: InstructionSlot, comment: String) {
        assert!(!self.inst_valid[slot.addr]);
        self.comments.insert(slot.addr, comment);
    }
    fn inst_comment(&mut self, inst: Instruction, comment: String) -> InstructionSlot {
        let slot = self.inst(inst);
        self.comment(slot, comment);
        slot
    }
    fn skip(&mut self) -> InstructionSlot {
        let addr = self.cursor;
        self.cursor += 1;
        InstructionSlot { addr }
    }

    fn func(&mut self, name: &'static str, f: impl FnOnce(&mut AssemblerInner)) {
        assert!(self.functions.get(name).is_none());
        self.functions
            .insert(name, InstructionSlot { addr: self.cursor });
        f(self);
    }

    fn finish(&self) -> Box<[Instruction; 65536]> {
        self.instructions.clone()
    }
}

fn addr_offset(from: usize, to: usize) -> (u8, u8, String) {
    let offset = to as i64 - from as i64;
    assert!(
        -128 <= offset && offset <= 127 && offset != 0,
        "offset: {}, from {}, to {}",
        offset,
        from,
        to
    );
    let comment = format!("--> to 0x{to:4x}");

    let offset = offset as i16 as u16;
    let hi = (offset >> 8) as u8;
    let lo = (offset & 0xff) as u8;
    (hi, lo, comment)
}
fn cond_to_jmp_inst(cond: Cond) -> fn(Imm4, Imm4) -> Instruction {
    let inst = match cond {
        Cond::Never => panic!("never"),
        Cond::Greater => j_offset_g,
        Cond::Equal => j_offset_e,
        Cond::Less => j_offset_l,
        Cond::GreaterEqual => j_offset_ge,
        Cond::LessEqual => j_offset_le,
        Cond::NotEqual => j_offset_ne,
        Cond::Always => j_offset,
    };
    inst
}
fn jmp_forward(asm: &mut AssemblerInner, base: InstructionSlot, cond: Cond) -> InstructionSlot {
    let (hi, lo, comment) = addr_offset(base.addr, asm.cursor);
    asm.comment(base, comment);

    let inst = cond_to_jmp_inst(cond);

    asm.inst_at(inst(lo, hi), base.addr)
}

/// jump/branch/call
impl AssemblerInner {
    pub fn jmp_forward(&mut self, cond: Cond) -> PendingJump {
        let cursor = self.cursor;
        self.skip();
        PendingJump {
            inst: jmp_forward,
            cond,
            addr: InstructionSlot { addr: cursor },
        }
    }
    fn resolve_jmp(&mut self, jmp: PendingJump) -> InstructionSlot {
        (jmp.inst)(self, jmp.addr, jmp.cond)
    }

    pub fn jmp_back(&mut self, target: InstructionSlot, cond: Cond) -> InstructionSlot {
        let (hi, lo, comment) = addr_offset(self.cursor, target.addr);
        let jmp_inst = cond_to_jmp_inst(cond);
        self.inst_comment(jmp_inst(lo, hi), comment)
    }

    pub fn jmp_reg(&mut self, target: Reg) -> InstructionSlot {
        self.inst_comment(jmp_reg(target), format!("--> jmp r{:x}", target))
    }
    pub fn call_reg(&mut self, target: Reg, back: Reg) -> InstructionSlot {
        self.inst_comment(
            call_reg(target, back),
            format!("--> call r{:x}, save r{:x}", target, back),
        )
    }

    pub fn if_u4(&mut self, reg0: Reg, u4: Imm4, cond: Cond, if_case: impl FnOnce(&mut Self)) {
        self.inst(cmp_i(u4, reg0));
        let skip_if = self.jmp_forward(cond);
        if_case(self);
        self.resolve_jmp(skip_if);
    }
    pub fn if_else_u4(
        &mut self,
        reg0: Reg,
        u4: Imm4,
        cond: Cond,
        if_case: impl FnOnce(&mut Self),
        else_case: impl FnOnce(&mut Self),
    ) {
        self.inst(cmp_i(u4, reg0));
        let skip_if = self.jmp_forward(cond);
        if_case(self);
        let skip_else = self.jmp_forward(Cond::Always);
        self.resolve_jmp(skip_if);
        else_case(self);
        self.resolve_jmp(skip_else);
    }
    pub fn if_reg(&mut self, reg0: Reg, reg1: Reg, cond: Cond, if_case: impl FnOnce(&mut Self)) {
        self.inst(cmp_r(reg1, reg0));
        let skip_if = self.jmp_forward(cond);
        if_case(self);
        self.resolve_jmp(skip_if);
    }
    pub fn if_else_reg(
        &mut self,
        reg0: Reg,
        reg1: Reg,
        cond: Cond,
        if_case: impl FnOnce(&mut Self),
        else_case: impl FnOnce(&mut Self),
    ) {
        self.inst(cmp_r(reg1, reg0));
        let skip_if = self.jmp_forward(cond);
        if_case(self);
        let skip_else = self.jmp_forward(Cond::Always);
        self.resolve_jmp(skip_if);
        else_case(self);
        self.resolve_jmp(skip_else);
    }
}

#[derive(Clone)]
pub struct Assembler {
    assembler: Rc<RefCell<AssemblerInner>>,
}
impl Assembler {
    pub fn new() -> Self {
        Self {
            assembler: Rc::new(RefCell::new(AssemblerInner::new())),
        }
    }

    pub fn reg(&self, reg: Reg) -> AsmRegister {
        assert!(reg < 16);
        AsmRegister {
            assembler: self.clone(),
            reg,
        }
    }
    pub fn mem(&self, reg: Reg) -> AsmMemoryAddr {
        assert!(reg < 16);
        AsmMemoryAddr {
            assembler: self.clone(),
            reg,
        }
    }
}

pub struct AsmRegister {
    assembler: Assembler,
    reg: Reg,
}
pub struct AsmMemoryAddr {
    assembler: Assembler,
    reg: Reg,
}
//TODO AsmRegister operators
//TODO AsmMemoryAddr operators
