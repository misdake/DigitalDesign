use crate::cpu_v1::isa::*;

#[derive(Copy, Clone)]
struct Instruction {
    comment: &'static str,
    data: u8,
    addr: u8,
}

struct Assembler {
    instructions: [Option<Instruction>; 256],
    functions: Vec<(u8, &'static str)>,
    cursor: usize,
}

impl Assembler {
    pub fn new() -> Self {
        Self {
            instructions: [None; 256],
            functions: vec![],
            cursor: 0,
        }
    }

    pub fn func(&mut self, name: &'static str, location_x16: u8) {}

    pub fn inst_comment(&mut self, inst: u8, comment: &'static str) {}
    pub fn inst(&mut self, inst: u8) {}

    pub fn reg0_mut(&mut self) {}
    pub fn reg1_mut(&mut self) {}
    pub fn reg2_mut(&mut self) {}
    pub fn reg3_mut(&mut self) {}

    pub fn jmp_offset(target: Instruction) {}
    pub fn je_offset(target: Instruction) {}
    pub fn jl_offset(target: Instruction) {}
    pub fn jg_offset(target: Instruction) {}
    pub fn jmp_long(target: u8) {}
    pub fn jmp_reg0() {}
    pub fn je_reg0() {}
    pub fn jl_reg0() {}
    pub fn jg_reg0() {}
    pub fn jmp_long_reg0() {}
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
    asembler: &'a mut Assembler,
}
struct Reg<'a> {
    asembler: &'a mut Assembler,
    reg_addr: RegisterIndex,
}
impl<'a> RegisterCommon for Reg0<'a> {
    fn assembler(&mut self) -> &mut Assembler {
        self.asembler
    }
    fn self_reg(&self) -> RegisterIndex {
        RegisterIndex::Reg0
    }
}
impl<'a> RegisterCommon for Reg<'a> {
    fn assembler(&mut self) -> &mut Assembler {
        self.asembler
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
