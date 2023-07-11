use crate::cpu_v1::isa::*;
use std::collections::HashMap;

#[derive(Copy, Clone)]
struct Instruction {
    data: u8,
    addr: usize,
}

struct Assembler {
    instructions: [Option<Instruction>; 256],
    function_names: HashMap<usize, &'static str>,
    function_addrs: HashMap<&'static str, usize>,
    comments: HashMap<usize, &'static str>,
    cursor: usize,
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

    pub fn func(&mut self, name: &'static str, addr_high: usize, f: impl FnOnce(&mut Assembler)) {
        assert!(self.function_names.insert(addr_high, name).is_none());
        assert!(self.function_addrs.insert(name, addr_high).is_none());
        self.cursor = addr_high * 16;
        f(self);
    }

    pub fn inst_comment(&mut self, inst: u8, comment: &'static str) -> Instruction {
        assert!(self.comments.insert(self.cursor, comment).is_none());
        self.inst(inst)
    }
    pub fn inst(&mut self, inst: u8) -> Instruction {
        assert!(self.instructions[self.cursor].is_none());
        let instruction = Instruction {
            data: inst,
            addr: self.cursor,
        };
        self.instructions[self.cursor] = Some(instruction);
        self.cursor += 1;
        instruction
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

    pub fn jmp_offset(&mut self, target: Instruction) {
        //TODO
    }
    pub fn je_offset(&mut self, target: Instruction) {
        //TODO
    }
    pub fn jl_offset(&mut self, target: Instruction) {
        //TODO
    }
    pub fn jg_offset(&mut self, target: Instruction) {
        //TODO
    }
    pub fn jmp_long(&mut self, function_name: &'static str) {
        let addr_high = *self
            .function_addrs
            .get(function_name)
            .expect("cannot find function name");
        self.inst(inst_jmp_long(addr_high as u8).binary);
    }
    pub fn jmp_offset_reg0(&mut self) {
        self.inst(inst_jmp_offset(0).binary);
    }
    pub fn je_offset_reg0(&mut self) {
        self.inst(inst_je_offset(0).binary);
    }
    pub fn jl_offset_reg0(&mut self) {
        self.inst(inst_jl_offset(0).binary);
    }
    pub fn jg_offset_reg0(&mut self) {
        self.inst(inst_jg_offset(0).binary);
    }
    pub fn jmp_long_reg0(&mut self) {
        self.inst(inst_jmp_long(0).binary);
    }
}

#[repr(u8)]
#[derive(Copy, Clone)]
enum RegisterIndex {
    Reg0 = 0,
    Reg1,
    Reg2,
    Reg3,
}
struct Reg0<'a> {
    assembler: &'a mut Assembler,
}
struct Reg<'a> {
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
impl<'a> RegisterCommon for Reg<'a> {
    fn assembler(&mut self) -> &mut Assembler {
        self.assembler
    }
    fn self_reg(&self) -> RegisterIndex {
        self.reg_addr
    }
}

trait RegisterCommon {
    fn assembler(&mut self) -> &mut Assembler;
    fn self_reg(&self) -> RegisterIndex;

    fn mov(&mut self, rhs: RegisterIndex) {
        let reg = self.self_reg() as u8;
        self.assembler().inst(inst_mov(rhs as u8, reg).binary);
    }
    fn and_assign(&mut self, rhs: RegisterIndex) {
        let reg = self.self_reg() as u8;
        self.assembler().inst(inst_and(rhs as u8, reg).binary);
    }
    fn or_assign(&mut self, rhs: RegisterIndex) {
        let reg = self.self_reg() as u8;
        self.assembler().inst(inst_or(rhs as u8, reg).binary);
    }
    fn xor_assign(&mut self, rhs: RegisterIndex) {
        let reg = self.self_reg() as u8;
        self.assembler().inst(inst_xor(rhs as u8, reg).binary);
    }
    fn add_assign(&mut self, rhs: RegisterIndex) {
        let reg = self.self_reg() as u8;
        self.assembler().inst(inst_add(rhs as u8, reg).binary);
    }
    fn inc(&mut self) {
        let reg = self.self_reg() as u8;
        self.assembler().inst(inst_inc(reg).binary);
    }
    fn dec(&mut self) {
        let reg = self.self_reg() as u8;
        self.assembler().inst(inst_dec(reg).binary);
    }
    fn inv(&mut self) {
        let reg = self.self_reg() as u8;
        self.assembler().inst(inst_inv(reg).binary);
    }
    fn neg(&mut self) {
        let reg = self.self_reg() as u8;
        self.assembler().inst(inst_neg(reg).binary);
    }
}

trait RegisterSpecial: RegisterCommon {
    fn load_imm(&mut self, imm: u8) {
        self.assembler().inst(inst_load_imm(imm).binary);
    }
    fn load_mem_imm(&mut self, imm: u8) {
        assert_ne!(imm, 0);
        self.assembler().inst(inst_load_mem(imm).binary);
    }
    fn load_mem_reg(&mut self) {
        self.assembler().inst(inst_load_mem(0).binary);
    }
    fn store_mem_imm(&mut self, imm: u8) {
        assert_ne!(imm, 0);
        self.assembler().inst(inst_store_mem(imm).binary);
    }
    fn store_mem_reg(&mut self) {
        self.assembler().inst(inst_store_mem(0).binary);
    }
}
