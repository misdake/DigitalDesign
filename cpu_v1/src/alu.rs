use crate::decoder::{Alu0Select, Alu1Select, AluOp};
use crate::CpuComponent;
use digital_design_code::{add_naive, Wires};

#[derive(Clone)]
pub struct CpuAluInput {
    // input
    pub reg0_data: Wires<4>,
    pub reg1_data: Wires<4>,
    pub imm: Wires<4>,

    // alu control
    pub alu_op: Wires<4>,      // AluOp: &, |, ^, +
    pub alu0_select: Wires<4>, // Alu0Select: reg0, ~reg0, 0, imm
    pub alu1_select: Wires<4>, // Alu1Select: reg1, -1, 0, 1
}
#[derive(Clone)]
pub struct CpuAluOutput {
    pub alu_out: Wires<4>,
}

pub struct CpuAlu;
impl CpuComponent for CpuAlu {
    type Input = CpuAluInput;
    type Output = CpuAluOutput;
    fn build(input: &Self::Input) -> Self::Output {
        let alu0_select = input.alu0_select.wires;
        let alu0_reg0 = input.reg0_data & alu0_select[Alu0Select::Reg0 as usize].expand();
        let alu0_reg0_inv = !input.reg0_data & alu0_select[Alu0Select::Reg0Inv as usize].expand();
        let alu0_zero = Wires::<4>::parse_u8(0) & alu0_select[Alu0Select::Zero as usize].expand();
        let alu0_imm = input.imm & alu0_select[Alu0Select::Imm as usize].expand();
        let alu0 = (alu0_reg0 | alu0_reg0_inv) | (alu0_zero | alu0_imm);

        let alu1_select = input.alu1_select.wires;
        let alu1_reg1 = input.reg1_data & alu1_select[Alu1Select::Reg1 as usize].expand();
        let alu1_neg_one =
            Wires::<4>::parse_u8(15) & alu1_select[Alu1Select::NegOne as usize].expand();
        let alu1_zero = Wires::<4>::parse_u8(0) & alu1_select[Alu1Select::Zero as usize].expand();
        let alu1_one = Wires::<4>::parse_u8(1) & alu1_select[Alu1Select::One as usize].expand();
        let alu1 = (alu1_reg1 | alu1_neg_one) | (alu1_zero | alu1_one);

        let op = input.alu_op.wires;
        let op_and = op[AluOp::And as usize].expand() & (alu0 & alu1);
        let op_or = op[AluOp::Or as usize].expand() & (alu0 | alu1);
        let op_xor = op[AluOp::Xor as usize].expand() & (alu0 ^ alu1);
        let op_add = op[AluOp::Add as usize].expand() & (add_naive(alu0, alu1).sum);
        let alu_out = (op_and | op_or) | (op_xor | op_add);

        CpuAluOutput { alu_out }
    }
}
