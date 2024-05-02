#![allow(clippy::manual_range_contains)]

use crate::isa::{halt, Instruction, Reg};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

#[derive(Copy, Clone)]
struct InstructionSlot {
    addr: usize,
}
struct PendingJump {
    inst: fn(&mut AssemblerInner, usize) -> InstructionSlot, // addr, target
    addr: usize,
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

impl AssemblerInner {
    fn addr_offset(from: usize, to: usize, name: &str) -> (u8, String) {
        let offset = to as i64 - from as i64;
        assert!(
            -128 <= offset && offset <= 127 && offset != 0,
            "offset: {}, cursor {}, target {}",
            offset,
            from,
            to
        );
        let offset = if offset < 0 {
            (offset + 16) as u8
        } else {
            offset as u8
        };
        let comment = format!("--> {}", name);
        (offset, comment)
    }

    //TODO jmps

    //TODO then if

    fn resolve_jmp(&mut self, jmp: PendingJump) -> InstructionSlot {
        (jmp.inst)(self, jmp.addr)
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
