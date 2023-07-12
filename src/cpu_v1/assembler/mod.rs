#![allow(unused)]

use crate::cpu_v1::isa::*;
use std::collections::HashMap;

#[derive(Copy, Clone)]
pub struct Instruction {
    data: InstBinary,
    addr: usize,
}

pub struct Assembler {
    instructions: [Option<Instruction>; 256],
    function_names: HashMap<usize, &'static str>,
    function_addrs: HashMap<&'static str, usize>,
    comments: HashMap<usize, &'static str>,

    cursor: usize,
}

#[test]
fn print() {
    let mut assembler = Assembler::new();
    assembler.func("main", 0, |assembler, _| {
        assembler.reg0().load_imm(10);
    });
    println!("{}", assembler.print());
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
                    format!("0b{:04b} {:04b}: ", i / 16, i % 16)
                } else {
                    format!("       {:04b}: ", i % 16)
                };
                let inst = inst
                    .map(|inst| inst.data.to_string())
                    .unwrap_or_else(|| "".to_string());
                let func = self
                    .function_names
                    .get(&i)
                    .map(|func_name| format!(" <--- {func_name}"))
                    .unwrap_or_else(|| "".to_string());
                let comment = self
                    .comments
                    .get(&i)
                    .map(|comment| format!(" // {comment}"))
                    .unwrap_or_else(|| "".to_string());

                format!("{addr}{inst}{func}{comment}")
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn finish(&self) -> [u8; 256] {
        self.instructions
            .map(|i| i.map(|i| i.data.binary).unwrap_or(0))
    }

    pub fn func(
        &mut self,
        name: &'static str,
        addr_high: usize,
        f: impl FnOnce(&mut Assembler, usize),
    ) {
        assert!(self.function_names.insert(addr_high, name).is_none());
        assert!(self.function_addrs.insert(name, addr_high).is_none());
        self.cursor = addr_high * 16;
        f(self, addr_high);
    }

    pub fn inst_comment(&mut self, inst: InstBinary, comment: &'static str) -> Instruction {
        assert!(self.comments.insert(self.cursor, comment).is_none());
        self.inst(inst)
    }
    pub fn inst(&mut self, inst: InstBinary) -> Instruction {
        let instruction = self.inst_at(self.cursor, inst);
        self.cursor += 1;
        instruction
    }
    pub fn inst_at(&mut self, addr: usize, inst: InstBinary) -> Instruction {
        assert!(self.instructions[addr].is_none());
        let instruction = Instruction { data: inst, addr };
        self.instructions[addr] = Some(instruction);
        instruction
    }
    pub fn skip_addr(&mut self) -> usize {
        let addr = self.cursor;
        self.cursor += 1;
        addr
    }

    pub fn reg0(&mut self) -> Reg0 {
        Reg0 { assembler: self }
    }
    pub fn reg1(&mut self) -> Reg {
        Reg {
            assembler: self,
            reg_addr: RegisterIndex::Reg1,
        }
    }
    pub fn reg2(&mut self) -> Reg {
        Reg {
            assembler: self,
            reg_addr: RegisterIndex::Reg2,
        }
    }
    pub fn reg3(&mut self) -> Reg {
        Reg {
            assembler: self,
            reg_addr: RegisterIndex::Reg3,
        }
    }

    fn addr_offset(&self, target: Instruction) -> u8 {
        let offset = target.addr as i64 - self.cursor as i64;
        assert!(
            -8 <= offset && offset < 7 && offset != 0,
            "offset: {}, cursor {}, target {}",
            offset,
            self.cursor,
            target.addr
        );
        if offset < 0 {
            (offset + 16) as u8
        } else {
            offset as u8
        }
    }
    pub fn jmp_offset(&mut self, target: Instruction) {
        let offset = self.addr_offset(target);
        self.inst(inst_jmp_offset(offset));
    }
    pub fn je_offset(&mut self, target: Instruction) {
        let offset = self.addr_offset(target);
        self.inst(inst_je_offset(offset));
    }
    pub fn jl_offset(&mut self, target: Instruction) {
        let offset = self.addr_offset(target);
        self.inst(inst_jl_offset(offset));
    }
    pub fn jg_offset(&mut self, target: Instruction) {
        let offset = self.addr_offset(target);
        self.inst(inst_jg_offset(offset));
    }
    pub fn jmp_long(&mut self, function_name: &'static str) {
        let addr_high = *self
            .function_addrs
            .get(function_name)
            .expect("cannot find function name");
        self.inst(inst_jmp_long(addr_high as u8));
    }
    pub fn jmp_offset_reg0(&mut self) {
        self.inst(inst_jmp_offset(0));
    }
    pub fn je_offset_reg0(&mut self) {
        self.inst(inst_je_offset(0));
    }
    pub fn jl_offset_reg0(&mut self) {
        self.inst(inst_jl_offset(0));
    }
    pub fn jg_offset_reg0(&mut self) {
        self.inst(inst_jg_offset(0));
    }
    // pub fn jmp_long_reg0(&mut self) { // probably not expected
    //     self.inst(inst_jmp_long(0));
    // }
}

pub struct PlaceHolder {
    addr: usize,
}

#[repr(u8)]
#[derive(Copy, Clone)]
pub enum RegisterIndex {
    Reg0 = 0,
    Reg1,
    Reg2,
    Reg3,
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
        RegisterIndex::Reg0
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

    fn mov(&mut self, rhs: RegisterIndex) {
        let reg = self.self_reg() as u8;
        self.assembler().inst(inst_mov(rhs as u8, reg));
    }
    fn and_assign(&mut self, rhs: RegisterIndex) {
        let reg = self.self_reg() as u8;
        self.assembler().inst(inst_and(rhs as u8, reg));
    }
    fn or_assign(&mut self, rhs: RegisterIndex) {
        let reg = self.self_reg() as u8;
        self.assembler().inst(inst_or(rhs as u8, reg));
    }
    fn xor_assign(&mut self, rhs: RegisterIndex) {
        let reg = self.self_reg() as u8;
        self.assembler().inst(inst_xor(rhs as u8, reg));
    }
    fn add_assign(&mut self, rhs: RegisterIndex) {
        let reg = self.self_reg() as u8;
        self.assembler().inst(inst_add(rhs as u8, reg));
    }
    fn inc(&mut self) {
        let reg = self.self_reg() as u8;
        self.assembler().inst(inst_inc(reg));
    }
    fn dec(&mut self) {
        let reg = self.self_reg() as u8;
        self.assembler().inst(inst_dec(reg));
    }
    fn inv(&mut self) {
        let reg = self.self_reg() as u8;
        self.assembler().inst(inst_inv(reg));
    }
    fn neg(&mut self) {
        let reg = self.self_reg() as u8;
        self.assembler().inst(inst_neg(reg));
    }
}

pub trait RegisterSpecial: RegisterCommon {
    fn load_imm(&mut self, imm: u8) {
        self.assembler().inst(inst_load_imm(imm));
    }
    fn load_mem_imm(&mut self, imm: u8) {
        assert_ne!(imm, 0);
        self.assembler().inst(inst_load_mem(imm));
    }
    fn load_mem_reg(&mut self) {
        self.assembler().inst(inst_load_mem(0));
    }
    fn store_mem_imm(&mut self, imm: u8) {
        assert_ne!(imm, 0);
        self.assembler().inst(inst_store_mem(imm));
    }
    fn store_mem_reg(&mut self) {
        self.assembler().inst(inst_store_mem(0));
    }
}
