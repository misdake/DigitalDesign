#![allow(unused)]

mod state;
pub use state::*;

use crate::cpu_v1::isa::Instruction::*;
use crate::cpu_v1::isa::RegisterIndex::*;
use crate::cpu_v1::isa::*;
use std::collections::HashMap;
use std::ops::Range;

#[derive(Copy, Clone)]
pub struct InstructionSlot {
    data: Instruction,
    addr: usize,
}
pub struct PendingJump {
    data: fn(&mut Assembler, usize) -> InstructionSlot, // addr, target
    addr: usize,
}
impl Assembler {
    pub fn resolve_jmp(&mut self, jmp: PendingJump) -> InstructionSlot {
        (jmp.data)(self, jmp.addr)
    }
}

pub struct Assembler {
    instructions: [Option<InstructionSlot>; 256],
    function_names: HashMap<usize, &'static str>,
    function_addrs: HashMap<&'static str, Range<usize>>,
    comments: HashMap<usize, String>,

    cursor: usize,
}

#[test]
fn print() {
    let mut asm = Assembler::new();
    asm.func("main", 0..1, |asm| {
        asm.reg0().load_imm(10);
    });
    asm.func("add", 1..2, |asm| {
        let i = asm.reg0().load_imm(12);
        asm.jmp_back(i);
    });
    println!("{}", asm.to_pretty_string());
}

impl Assembler {
    pub fn new() -> Self {
        Self {
            instructions: [None; 256],
            function_names: HashMap::new(),
            function_addrs: HashMap::new(),
            comments: HashMap::new(),
            cursor: 0,
        }
    }

    pub fn to_pretty_string(&self) -> String {
        self.instructions
            .iter()
            .enumerate()
            .map(|(i, inst)| {
                let addr = if i % 16 == 0 {
                    format!("{:3} {:04b}: ", i / 16, i % 16)
                } else {
                    format!("    {:04b}: ", i % 16)
                };
                let inst = inst
                    .map(|inst| inst.data.to_string())
                    .unwrap_or_else(|| "".to_string());
                let func = self
                    .function_names
                    .get(&i)
                    .map(|func_name| format!(" <-- fn {func_name}"))
                    .unwrap_or_else(|| "".to_string());
                let comment = self
                    .comments
                    .get(&i)
                    .map(|comment| format!(" {comment}"))
                    .unwrap_or_else(|| "".to_string());

                format!("{addr}{inst:22}{func}{comment}")
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn finish(&self) -> [Instruction; 256] {
        self.instructions
            .map(|i| i.map(|i| i.data).unwrap_or(Instruction::default()))
    }

    pub fn func_decl(&mut self, name: &'static str, addr_high: Range<usize>) {
        assert!(!addr_high.is_empty() && addr_high.start < 16 && addr_high.end <= 16);
        assert!(self
            .function_names
            .insert(addr_high.start * 16, name)
            .is_none());
        assert!(self.function_addrs.insert(name, addr_high).is_none());
    }
    pub fn func_impl(&mut self, name: &'static str, f: impl FnOnce(&mut Assembler)) {
        let addr = self.function_addrs.get(name).unwrap();
        let start = addr.start;
        let end = addr.end;
        self.cursor = start * 16;
        f(self);
        assert!(self.cursor <= end * 16);
    }
    pub fn func(
        &mut self,
        name: &'static str,
        addr_high: Range<usize>,
        f: impl FnOnce(&mut Assembler),
    ) {
        self.func_decl(name, addr_high);
        self.func_impl(name, f);
    }

    pub fn comment(&mut self, comment: String) {
        assert!(self.comments.insert(self.cursor, comment).is_none());
    }
    pub fn comment_at(&mut self, addr: usize, comment: String) {
        assert!(self.comments.insert(addr, comment).is_none());
    }
    pub fn inst_comment(&mut self, inst: Instruction, comment: String) -> InstructionSlot {
        assert!(self.comments.insert(self.cursor, comment).is_none());
        self.inst(inst)
    }
    pub fn inst(&mut self, inst: Instruction) -> InstructionSlot {
        let instruction = self.inst_at(self.cursor, inst);
        self.cursor += 1;
        instruction
    }
    pub fn inst_at(&mut self, addr: usize, inst: Instruction) -> InstructionSlot {
        assert!(self.instructions[addr].is_none());
        let instruction = InstructionSlot { data: inst, addr };
        self.instructions[addr] = Some(instruction);
        instruction
    }
    pub fn skip_addr(&mut self) -> usize {
        let addr = self.cursor;
        self.cursor += 1;
        addr
    }
    pub fn set_cursor(&mut self, cursor: usize) {
        self.cursor = cursor;
    }

    pub fn reg0(&mut self) -> Reg0 {
        Reg0 { assembler: self }
    }
    pub fn reg1(&mut self) -> Reg {
        Reg {
            assembler: self,
            reg_addr: Reg1,
        }
    }
    pub fn reg2(&mut self) -> Reg {
        Reg {
            assembler: self,
            reg_addr: Reg2,
        }
    }
    pub fn reg3(&mut self) -> Reg {
        Reg {
            assembler: self,
            reg_addr: Reg3,
        }
    }

    fn addr_offset(cursor: usize, target: usize) -> (u8, String) {
        let offset = target as i64 - cursor as i64;
        assert!(
            -8 <= offset && offset <= 7 && offset != 0,
            "offset: {}, cursor {}, target {}",
            offset,
            cursor,
            target
        );
        let offset = if offset < 0 {
            (offset + 16) as u8
        } else {
            offset as u8
        };
        let comment = format!("--> {:3} {:04b}", target / 16, target % 16);
        (offset, comment)
    }

    pub fn jmp_forward(&mut self) -> PendingJump {
        fn jmp_forward(asm: &mut Assembler, base: usize) -> InstructionSlot {
            let (offset, comment) = Assembler::addr_offset(base, asm.cursor);
            asm.comment_at(base, comment);
            asm.inst_at(base, jmp_offset(offset))
        }
        let cursor = self.cursor;
        self.skip_addr();
        PendingJump {
            data: jmp_forward,
            addr: cursor,
        }
    }
    pub fn jne_forward(&mut self) -> PendingJump {
        fn jne_forward(asm: &mut Assembler, base: usize) -> InstructionSlot {
            let (offset, comment) = Assembler::addr_offset(base, asm.cursor);
            asm.comment_at(base, comment);
            asm.inst_at(base, jne_offset(offset))
        }
        let cursor = self.cursor;
        self.skip_addr();
        PendingJump {
            data: jne_forward,
            addr: cursor,
        }
    }
    pub fn jl_forward(&mut self) -> PendingJump {
        fn jl_forward(asm: &mut Assembler, base: usize) -> InstructionSlot {
            let (offset, comment) = Assembler::addr_offset(base, asm.cursor);
            asm.comment_at(base, comment);
            asm.inst_at(base, jl_offset(offset))
        }
        let cursor = self.cursor;
        self.skip_addr();
        PendingJump {
            data: jl_forward,
            addr: cursor,
        }
    }
    pub fn jg_forward(&mut self) -> PendingJump {
        fn jg_forward(asm: &mut Assembler, base: usize) -> InstructionSlot {
            let (offset, comment) = Assembler::addr_offset(base, asm.cursor);
            asm.comment_at(base, comment);
            asm.inst_at(base, jg_offset(offset))
        }
        let cursor = self.cursor;
        self.skip_addr();
        PendingJump {
            data: jg_forward,
            addr: cursor,
        }
    }

    pub fn jmp_back(&mut self, target: InstructionSlot) -> InstructionSlot {
        let (offset, comment) = Self::addr_offset(self.cursor, target.addr);
        self.inst_comment(jmp_offset(offset), comment)
    }
    pub fn jne_back(&mut self, target: InstructionSlot) -> InstructionSlot {
        let (offset, comment) = Self::addr_offset(self.cursor, target.addr);
        self.inst_comment(jne_offset(offset), comment)
    }
    pub fn jl_back(&mut self, target: InstructionSlot) -> InstructionSlot {
        let (offset, comment) = Self::addr_offset(self.cursor, target.addr);
        self.inst_comment(jl_offset(offset), comment)
    }
    pub fn jg_back(&mut self, target: InstructionSlot) -> InstructionSlot {
        let (offset, comment) = Self::addr_offset(self.cursor, target.addr);
        self.inst_comment(jg_offset(offset), comment)
    }

    pub fn jmp_long(&mut self, function_name: &'static str) {
        let addr = self
            .function_addrs
            .get(function_name)
            .expect("cannot find function name")
            .clone();
        if addr.start == 0 {
            self.reg0().load_imm(0);
            self.inst(jmp_long(0));
        } else {
            self.inst(jmp_long(addr.start as u8));
        }
    }

    pub fn jmp_offset_reg0(&mut self) {
        self.inst(jmp_offset(0));
    }
    pub fn jne_offset_reg0(&mut self) {
        self.inst(jne_offset(0));
    }
    pub fn jl_offset_reg0(&mut self) {
        self.inst(jl_offset(0));
    }
    pub fn jg_offset_reg0(&mut self) {
        self.inst(jg_offset(0));
    }

    pub fn bus0(&mut self, op3: u8) -> InstructionSlot {
        self.inst(bus0(op3))
    }
    pub fn bus1(&mut self, op3: u8) -> InstructionSlot {
        self.inst(bus1(op3))
    }
}

pub struct PlaceHolder {
    addr: usize,
}

pub struct Reg0<'a> {
    assembler: &'a mut Assembler,
}
pub struct Reg<'a> {
    assembler: &'a mut Assembler,
    reg_addr: RegisterIndex,
}
impl<'a> RegisterCommon for Reg0<'a> {
    fn assembler(&mut self) -> &mut Assembler {
        self.assembler
    }
    fn self_reg(&self) -> RegisterIndex {
        Reg0
    }
}
impl<'a> RegisterSpecial for Reg0<'a> {}
impl<'a> RegisterCommon for Reg<'a> {
    fn assembler(&mut self) -> &mut Assembler {
        self.assembler
    }
    fn self_reg(&self) -> RegisterIndex {
        self.reg_addr
    }
}

pub trait RegisterCommon {
    fn assembler(&mut self) -> &mut Assembler;
    fn self_reg(&self) -> RegisterIndex;

    fn assign_from(&mut self, rhs: RegisterIndex) -> InstructionSlot {
        let reg = self.self_reg();
        self.assembler().inst(mov((rhs, reg)))
    }
    fn and_assign(&mut self, rhs: RegisterIndex) -> InstructionSlot {
        let reg = self.self_reg();
        self.assembler().inst(and((rhs, reg)))
    }
    fn or_assign(&mut self, rhs: RegisterIndex) -> InstructionSlot {
        let reg = self.self_reg();
        self.assembler().inst(or((rhs, reg)))
    }
    fn xor_assign(&mut self, rhs: RegisterIndex) -> InstructionSlot {
        let reg = self.self_reg();
        self.assembler().inst(xor((rhs, reg)))
    }
    fn add_assign(&mut self, rhs: RegisterIndex) -> InstructionSlot {
        let reg = self.self_reg();
        self.assembler().inst(add((rhs, reg)))
    }
    fn inc(&mut self) -> InstructionSlot {
        let reg = self.self_reg();
        self.assembler().inst(inc(reg))
    }
    fn dec(&mut self) -> InstructionSlot {
        let reg = self.self_reg();
        self.assembler().inst(dec(reg))
    }
    fn inv(&mut self) -> InstructionSlot {
        let reg = self.self_reg();
        self.assembler().inst(inv(reg))
    }
    fn neg(&mut self) -> InstructionSlot {
        let reg = self.self_reg();
        self.assembler().inst(neg(reg))
    }
}

pub trait RegisterSpecial: RegisterCommon {
    fn load_imm(&mut self, imm: u8) -> InstructionSlot {
        self.assembler().inst(load_imm(imm))
    }
    fn load_mem_imm(&mut self, imm: u8) -> InstructionSlot {
        assert_ne!(imm, 0);
        self.assembler().inst(load_mem(imm))
    }
    fn load_mem_reg(&mut self) -> InstructionSlot {
        self.assembler().inst(load_mem(0))
    }
    fn store_mem_imm(&mut self, imm: u8) -> InstructionSlot {
        assert_ne!(imm, 0);
        self.assembler().inst(store_mem(imm))
    }
    fn store_mem_reg(&mut self) -> InstructionSlot {
        self.assembler().inst(store_mem(0))
    }
    fn set_mem_page(&mut self) -> InstructionSlot {
        self.assembler().inst(set_mem_page(()))
    }
    fn set_bus_addr0(&mut self) -> InstructionSlot {
        self.assembler().inst(set_bus_addr0(()))
    }
    fn set_bus_addr1(&mut self) -> InstructionSlot {
        self.assembler().inst(set_bus_addr1(()))
    }
}
