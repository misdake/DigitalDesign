#![allow(unused)]

mod state;
pub use state::*;

use crate::cpu_v1::isa::Instruction::*;
use crate::cpu_v1::isa::RegisterIndex::*;
use crate::cpu_v1::isa::*;
use std::collections::HashMap;

#[derive(Copy, Clone)]
pub struct InstructionType {
    data: Instruction,
    addr: usize,
}

pub struct Assembler {
    instructions: [Option<InstructionType>; 256],
    function_names: HashMap<usize, &'static str>,
    function_addrs: HashMap<&'static str, usize>,
    comments: HashMap<usize, String>,

    cursor: usize,
}

#[test]
fn print() {
    let mut asm = Assembler::new();
    asm.func("main", 0, |asm, _| {
        asm.reg0().load_imm(10);
    });
    asm.func("add", 1, |asm, _| {
        let i = asm.reg0().load_imm(12);
        asm.jmp_offset(i);
    });
    println!("{}", asm.print());
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

    pub fn print(&self) -> String {
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
                    .map(|func_name| format!(" <-- {func_name}"))
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

    pub fn func(
        &mut self,
        name: &'static str,
        addr_high: usize,
        f: impl FnOnce(&mut Assembler, usize),
    ) {
        let addr = addr_high * 16;
        assert!(self.function_names.insert(addr, name).is_none());
        assert!(self.function_addrs.insert(name, addr).is_none());
        self.cursor = addr;
        f(self, addr);
    }

    pub fn inst_comment(&mut self, inst: Instruction, comment: String) -> InstructionType {
        assert!(self.comments.insert(self.cursor, comment).is_none());
        self.inst(inst)
    }
    pub fn inst(&mut self, inst: Instruction) -> InstructionType {
        let instruction = self.inst_at(self.cursor, inst);
        self.cursor += 1;
        instruction
    }
    pub fn inst_at(&mut self, addr: usize, inst: Instruction) -> InstructionType {
        assert!(self.instructions[addr].is_none());
        let instruction = InstructionType { data: inst, addr };
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

    fn addr_offset(&self, target: InstructionType) -> (u8, String) {
        let offset = target.addr as i64 - self.cursor as i64;
        assert!(
            -8 <= offset && offset <= 7 && offset != 0,
            "offset: {}, cursor {}, target {}",
            offset,
            self.cursor,
            target.addr
        );
        let offset = if offset < 0 {
            (offset + 16) as u8
        } else {
            offset as u8
        };
        let comment = format!("--> {:3} {:04b}", target.addr / 16, target.addr % 16);
        (offset, comment)
    }
    pub fn jmp_offset(&mut self, target: InstructionType) {
        let (offset, comment) = self.addr_offset(target);
        self.inst_comment(jmp_offset(offset), comment);
    }
    pub fn jne_offset(&mut self, target: InstructionType) {
        let (offset, comment) = self.addr_offset(target);
        self.inst_comment(jne_offset(offset), comment);
    }
    pub fn jl_offset(&mut self, target: InstructionType) {
        let (offset, comment) = self.addr_offset(target);
        self.inst_comment(jl_offset(offset), comment);
    }
    pub fn jg_offset(&mut self, target: InstructionType) {
        let (offset, comment) = self.addr_offset(target);
        self.inst_comment(jg_offset(offset), comment);
    }
    pub fn jmp_long(&mut self, function_name: &'static str) {
        let addr = *self
            .function_addrs
            .get(function_name)
            .expect("cannot find function name");
        assert_eq!(addr % 16, 0);
        self.inst(jmp_long(addr as u8 % 16));
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
    // pub fn jmp_long_reg0(&mut self) { // probably not expected
    //     self.inst(jmp_long(0));
    // }
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

    fn assign_from(&mut self, rhs: RegisterIndex) -> InstructionType {
        let reg = self.self_reg();
        self.assembler().inst(mov((rhs, reg)))
    }
    fn and_assign(&mut self, rhs: RegisterIndex) -> InstructionType {
        let reg = self.self_reg();
        self.assembler().inst(and((rhs, reg)))
    }
    fn or_assign(&mut self, rhs: RegisterIndex) -> InstructionType {
        let reg = self.self_reg();
        self.assembler().inst(or((rhs, reg)))
    }
    fn xor_assign(&mut self, rhs: RegisterIndex) -> InstructionType {
        let reg = self.self_reg();
        self.assembler().inst(xor((rhs, reg)))
    }
    fn add_assign(&mut self, rhs: RegisterIndex) -> InstructionType {
        let reg = self.self_reg();
        self.assembler().inst(add((rhs, reg)))
    }
    fn inc(&mut self) -> InstructionType {
        let reg = self.self_reg();
        self.assembler().inst(inc(reg))
    }
    fn dec(&mut self) -> InstructionType {
        let reg = self.self_reg();
        self.assembler().inst(dec(reg))
    }
    fn inv(&mut self) -> InstructionType {
        let reg = self.self_reg();
        self.assembler().inst(inv(reg))
    }
    fn neg(&mut self) -> InstructionType {
        let reg = self.self_reg();
        self.assembler().inst(neg(reg))
    }
}

pub trait RegisterSpecial: RegisterCommon {
    fn load_imm(&mut self, imm: u8) -> InstructionType {
        self.assembler().inst(load_imm(imm))
    }
    fn load_mem_imm(&mut self, imm: u8) -> InstructionType {
        assert_ne!(imm, 0);
        self.assembler().inst(load_mem(imm))
    }
    fn load_mem_reg(&mut self) -> InstructionType {
        self.assembler().inst(load_mem(0))
    }
    fn store_mem_imm(&mut self, imm: u8) -> InstructionType {
        assert_ne!(imm, 0);
        self.assembler().inst(store_mem(imm))
    }
    fn store_mem_reg(&mut self) -> InstructionType {
        self.assembler().inst(store_mem(0))
    }
}
