use crate::cpu_v1::{CpuComponent, CpuComponentEmu, INST_INC};
use crate::{cpu_v1, input, input_w, Wire, Wires};

#[derive(Clone)]
pub struct CpuDecoderInput {
    pub inst: Wires<8>,
}
#[derive(Clone)]
pub struct CpuDecoderOutput {
    pub imm: Wires<4>,

    // reg
    pub reg0_addr: Wires<2>,
    pub reg1_addr: Wires<2>,
    pub reg0_write_enable: Wire,
    pub reg0_write_select: Wires<4>, // alu out, mem out

    // alu
    pub alu_op: Wires<4>,      // &, |, ^, +
    pub alu0_select: Wires<4>, // reg0, ~reg0, 0, imm
    pub alu1_select: Wires<4>, // reg1, -1, 0, 1

    // mem
    pub mem_addr_select: Wires<2>, // imm, reg1
    pub mem_addr: Wires<4>,
    pub mem_write_enable: Wire,

    // branch
    //TODO
    pub flag_write_enable: Wire,
}

#[repr(u8)]
pub enum AluOp {
    And,
    Or,
    Xor,
    Add,
}
#[repr(u8)]
pub enum Alu0Select {
    Reg0 = 0,
    Reg0Inv = 1,
    Zero = 2,
    Imm = 3,
}
#[repr(u8)]
pub enum Alu1Select {
    Reg1 = 0,
    NegOne = 1,
    Zero = 2,
    One = 3,
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
            mem_addr: input_w(),
            mem_write_enable: input(),
            flag_write_enable: input(),
        }
    }
    fn execute(input: &CpuDecoderInput, output: &CpuDecoderOutput) {
        use cpu_v1::isa::*;
        let inst = input.inst.get_u8();
        let mov = INST_MOV.match_opcode(inst);
        let and = INST_AND.match_opcode(inst);
        let or = INST_OR.match_opcode(inst);
        let xor = INST_XOR.match_opcode(inst);
        let add = INST_ADD.match_opcode(inst);
        let inv = INST_INV.match_opcode(inst);
        let neg = INST_NEG.match_opcode(inst);
        let inc = INST_INC.match_opcode(inst);
        let dec = INST_DEC.match_opcode(inst);

        let reg0 = (inst & 0b00000011) >> 0;
        let reg1 = (inst & 0b00001100) >> 2;
        output.reg0_addr.set_u8(reg0);
        output.reg1_addr.set_u8(reg1);

        let is_alu_op2 = mov || and || or || xor || add;
        let is_alu_op1 = inv || neg || inc || dec;
        let is_alu = is_alu_op2 || is_alu_op1;

        // set alu_op, alu0_select, alu1_select
        if is_alu {
            output.reg0_write_enable.set(1);
            output.flag_write_enable.set(1);

            let set_alu = |op_match: bool, op: AluOp, alu0: Alu0Select, alu1: Alu1Select| {
                if op_match {
                    output.alu_op.set_u8(op as u8);
                    output.alu0_select.set_u8(alu0 as u8);
                    output.alu1_select.set_u8(alu1 as u8);
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
        }

        //TODO other output
    }
}
