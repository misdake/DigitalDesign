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
enum Register {
    Reg0,
    Reg1,
    Reg2,
    Reg3,
}
struct Reg<'a> {
    asembler: &'a mut Assembler,
    reg_addr: Register,
}

trait RegisterCommon {
    fn mov(&mut self, rhs: Register) {}
    fn and_assign(&mut self, rhs: Register) {}
    fn or_assign(&mut self, rhs: Register) {}
    fn xor_assign(&mut self, rhs: Register) {}
    fn add_assign(&mut self, rhs: Register) {}
    fn inc(&mut self) {}
    fn dec(&mut self) {}
    fn inv(&mut self) {}
    fn neg(&mut self) {}
}
trait Register0 {
    fn load_imm(&mut self, imm: u8) {}
    fn load_mem_imm(&mut self, imm: u8) {}
    fn load_mem_reg(&mut self) {}
    fn store_mem_imm(&mut self, imm: u8) {}
    fn store_mem_reg(&mut self) {}
}
