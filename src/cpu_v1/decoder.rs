use crate::cpu_v1::{CpuComponent, CpuComponentEmu, INST_INC};
use crate::{cpu_v1, input, input_w, Wire, Wires};

#[derive(Clone)]
pub struct CpuDecoderInput {
    pub inst: Wires<8>,
}
#[derive(Clone)]
pub struct CpuDecoderOutput {
    // data output
    pub reg0_addr: Wires<2>, // RegAddr
    pub reg1_addr: Wires<2>, // RegAddr
    pub imm: Wires<4>,

    // reg control
    pub reg0_write_enable: Wire,
    pub reg0_write_select: Wires<2>, // Reg0WriteSelect: alu out, mem out

    // alu control
    pub alu_op: Wires<4>,      // AluOp: &, |, ^, +
    pub alu0_select: Wires<4>, // Alu0Select: reg0, ~reg0, 0, imm
    pub alu1_select: Wires<4>, // Alu1Select: reg1, -1, 0, 1

    // mem control
    pub mem_addr_select: Wires<2>, // MemAddrSelect: imm, reg1
    pub mem_write_enable: Wire,

    // jmp control
    pub jmp_op: Wires<6>,         // JmpOp: no_jmp, jmp, je, jl, jg, long
    pub jmp_src_select: Wires<2>, // JmpSrcSelect: imm, regA
}

#[repr(u8)]
enum RegAddr {
    A = 0,
    B = 1,
    C = 2,
    D = 3,
}
#[repr(u8)]
enum Reg0WriteSelect {
    AluOut = 1,
    MemOut = 2,
}

#[repr(u8)]
enum AluOp {
    And = 1,
    Or = 2,
    Xor = 4,
    Add = 8,
}
#[repr(u8)]
enum Alu0Select {
    Reg0 = 1,
    Reg0Inv = 2,
    Zero = 4,
    Imm = 8,
}
#[repr(u8)]
enum Alu1Select {
    Reg1 = 1,
    NegOne = 2,
    Zero = 4,
    One = 8,
}

#[repr(u8)]
enum MemAddrSelect {
    Imm = 1,
    Reg1 = 2,
}

#[allow(unused)]
#[repr(u8)]
enum JmpOp {
    NoJmp = 1,
    Jmp = 2,
    Je = 4,
    Jl = 8,
    Jg = 16,
    Long = 32,
}
#[repr(u8)]
enum JmpSrcSelect {
    Imm = 1,
    RegA = 2,
}

pub struct CpuDecoder;
impl CpuComponent for CpuDecoder {
    type Input = CpuDecoderInput;
    type Output = CpuDecoderOutput;

    fn build(_input: &CpuDecoderInput) -> CpuDecoderOutput {
        todo!()
    }
}

pub struct CpuDecoderEmu;
impl CpuComponentEmu<CpuDecoder> for CpuDecoderEmu {
    fn init_output() -> CpuDecoderOutput {
        CpuDecoderOutput {
            imm: input_w(),
            reg0_addr: input_w(),
            reg1_addr: input_w(),

            reg0_write_enable: input(),
            reg0_write_select: input_w(),
            alu_op: input_w(),
            alu0_select: input_w(),
            alu1_select: input_w(),
            mem_addr_select: input_w(),
            mem_write_enable: input(),
            jmp_op: input_w(),
            jmp_src_select: input_w(),
        }
    }
    fn execute(input: &CpuDecoderInput, output: &CpuDecoderOutput) {
        use cpu_v1::isa::*;
        let inst = input.inst.get_u8();

        let reg0_bits: u8 = (inst & 0b00000011) >> 0;
        let reg1_bits: u8 = (inst & 0b00001100) >> 2;
        let imm: u8 = (inst & 0b00001111) >> 0;

        // alu_op2
        let mov = INST_MOV.match_opcode(inst);
        let and = INST_AND.match_opcode(inst);
        let or = INST_OR.match_opcode(inst);
        let xor = INST_XOR.match_opcode(inst);
        let add = INST_ADD.match_opcode(inst);
        // alu_op1
        let inv = INST_INV.match_opcode(inst);
        let neg = INST_NEG.match_opcode(inst);
        let inc = INST_INC.match_opcode(inst);
        let dec = INST_DEC.match_opcode(inst);
        // load store
        let load_imm = INST_LOAD_IMM.match_opcode(inst);
        let load_mem_imm = INST_LOAD_MEM.match_opcode(inst) && (imm != 0);
        let load_mem_reg = INST_LOAD_MEM.match_opcode(inst) && (imm == 0);
        let store_mem_imm = INST_STORE_MEM.match_opcode(inst) && (imm != 0);
        let store_mem_reg = INST_STORE_MEM.match_opcode(inst) && (imm == 0);
        // jmp
        let jmp_offset = INST_JMP_OFFSET.match_opcode(inst);
        let je_offset = INST_JE_OFFSET.match_opcode(inst);
        let jl_offset = INST_JL_OFFSET.match_opcode(inst);
        let jg_offset = INST_JG_OFFSET.match_opcode(inst);
        let jmp_long = INST_JMP_LONG.match_opcode(inst);
        // control TODO
        // let reset = INST_RESET.match_opcode(inst);
        // let halt = INST_HALT.match_opcode(inst);

        // immutable local variable => all output variables assigned once and only once.
        let reg0_addr: u8;
        let reg1_addr: u8;
        let reg0_write_enable: u8;
        let reg0_write_select: u8;
        let alu_op: u8;
        let alu0_select: u8;
        let alu1_select: u8;
        let mem_addr_select: u8;
        let mem_write_enable: u8;
        let jmp_op: u8;
        let jmp_src_select: u8;

        let is_alu = mov || and || or || xor || add || inv || neg || inc || dec;
        let is_load_imm = load_imm;
        let is_load_mem = load_mem_imm || load_mem_reg;
        let is_store_mem = store_mem_imm || store_mem_reg;
        let is_jmp = jmp_offset || je_offset || jl_offset || jg_offset || jmp_long;

        if is_alu || is_load_imm {
            jmp_op = JmpOp::NoJmp as u8;
            jmp_src_select = JmpSrcSelect::Imm as u8;
            reg0_addr = reg0_bits;
            reg1_addr = reg1_bits;
            reg0_write_enable = 1;
            reg0_write_select = Reg0WriteSelect::AluOut as u8;
            mem_addr_select = 0;
            mem_write_enable = 0;

            let mut v_alu_op: u8 = 0;
            let mut v_alu0_select: u8 = 0;
            let mut v_alu1_select: u8 = 0;
            let mut set_alu = |op_match: bool, op: AluOp, alu0: Alu0Select, alu1: Alu1Select| {
                if op_match {
                    v_alu_op = op as u8;
                    v_alu0_select = alu0 as u8;
                    v_alu1_select = alu1 as u8;
                }
            };

            set_alu(mov, AluOp::Or, Alu0Select::Zero, Alu1Select::Reg1);
            set_alu(and, AluOp::And, Alu0Select::Reg0, Alu1Select::Reg1);
            set_alu(or, AluOp::Or, Alu0Select::Reg0, Alu1Select::Reg1);
            set_alu(xor, AluOp::Xor, Alu0Select::Reg0, Alu1Select::Reg1);
            set_alu(add, AluOp::Add, Alu0Select::Reg0, Alu1Select::Reg1);

            set_alu(inv, AluOp::Or, Alu0Select::Reg0Inv, Alu1Select::Zero);
            set_alu(neg, AluOp::Add, Alu0Select::Reg0Inv, Alu1Select::One);
            set_alu(inc, AluOp::Add, Alu0Select::Reg0, Alu1Select::One);
            set_alu(dec, AluOp::Add, Alu0Select::Reg0, Alu1Select::NegOne);
            set_alu(load_imm, AluOp::Or, Alu0Select::Imm, Alu1Select::Zero);

            alu_op = v_alu_op;
            alu0_select = v_alu0_select;
            alu1_select = v_alu1_select;
        } else if is_load_mem || is_store_mem {
            jmp_op = JmpOp::NoJmp as u8;
            jmp_src_select = JmpSrcSelect::Imm as u8;
            reg0_addr = RegAddr::A as u8;
            reg1_addr = RegAddr::B as u8;
            alu_op = 0;
            alu0_select = 0;
            alu1_select = 0;
            if is_load_mem {
                reg0_write_enable = 1;
                reg0_write_select = Reg0WriteSelect::MemOut as u8;
                mem_write_enable = 0;
                if load_mem_imm {
                    mem_addr_select = MemAddrSelect::Imm as u8;
                } else {
                    mem_addr_select = MemAddrSelect::Reg1 as u8;
                }
            } else if is_store_mem {
                reg0_write_enable = 0;
                reg0_write_select = 0;
                mem_write_enable = 1;
                if store_mem_imm {
                    mem_addr_select = MemAddrSelect::Imm as u8;
                } else {
                    mem_addr_select = MemAddrSelect::Reg1 as u8;
                }
            } else {
                unreachable!()
            }
        } else if is_jmp {
            jmp_op = (jmp_offset as u8 >> 1)
                | (je_offset as u8 >> 2)
                | (jl_offset as u8 >> 3)
                | (jg_offset as u8 >> 4)
                | (jmp_long as u8 >> 5);
            reg0_addr = 0; // used in, unused otherwise
            reg1_addr = 0;
            reg0_write_enable = 0;
            reg0_write_select = 0;
            alu_op = 0;
            alu0_select = 0;
            alu1_select = 0;
            mem_addr_select = 0;
            mem_write_enable = 0;
            jmp_src_select = if imm == 0 {
                JmpSrcSelect::RegA as u8
            } else {
                JmpSrcSelect::Imm as u8
            };
        } else {
            //TODO control
            //TODO external
            unimplemented!()
        }

        output.imm.set_u8(imm);
        output.reg0_addr.set_u8(reg0_addr);
        output.reg1_addr.set_u8(reg1_addr);
        output.reg0_write_enable.set(reg0_write_enable);
        output.reg0_write_select.set_u8(reg0_write_select);
        output.alu_op.set_u8(alu_op);
        output.alu0_select.set_u8(alu0_select);
        output.alu1_select.set_u8(alu1_select);
        output.mem_addr_select.set_u8(mem_addr_select);
        output.mem_write_enable.set(mem_write_enable);
        output.jmp_op.set_u8(jmp_op);
        output.jmp_src_select.set_u8(jmp_src_select);
    }
}
