use crate::cpu_v1::decoder::{JmpOp, JmpSrcSelect};
use crate::cpu_v1::CpuComponent;
use crate::{mux2, Wire, Wires};

#[derive(Debug, Clone)]
pub struct CpuBranchInput {
    pub imm: Wires<4>,
    pub reg0: Wires<4>,
    pub reg0_write_enable: Wire,
    pub reg0_write_data: Wires<4>,

    pub jmp_op: Wires<6>,         // JmpOp: no_jmp, jmp, je, jl, jg, long
    pub jmp_src_select: Wires<2>, // JmpSrcSelect: imm, regA

    pub flag_p: Wire,
    pub flag_z: Wire,
    pub flag_n: Wire,
}
#[derive(Clone)]
pub struct CpuBranchOutput {
    pub pc_offset_enable: Wire,
    pub pc_offset: Wires<4>,
    pub jmp_long_enable: Wire,
    pub jmp_long: Wires<4>,
    pub flag_p: Wire,
    pub flag_z: Wire,
    pub flag_n: Wire,
}

pub struct CpuBranch;
impl CpuComponent for CpuBranch {
    type Input = CpuBranchInput;
    type Output = CpuBranchOutput;
    fn build(input: &Self::Input) -> Self::Output {
        let no_jmp = input.jmp_op.wires[JmpOp::NoJmp as usize]
            | (input.jmp_op.wires[JmpOp::Je as usize] & !input.flag_z)
            | (input.jmp_op.wires[JmpOp::Jl as usize] & !input.flag_n)
            | (input.jmp_op.wires[JmpOp::Jg as usize] & !input.flag_p);
        let jmp = input.jmp_op.wires[JmpOp::Jmp as usize];
        let je = input.jmp_op.wires[JmpOp::Je as usize] & input.flag_z;
        let jl = input.jmp_op.wires[JmpOp::Jl as usize] & input.flag_n;
        let jg = input.jmp_op.wires[JmpOp::Jg as usize] & input.flag_p;
        let jmp_long = input.jmp_op.wires[JmpOp::Long as usize];

        let jmp_src_imm = !no_jmp & input.jmp_src_select.wires[JmpSrcSelect::Imm as usize];
        let jmp_src_reg = !no_jmp & input.jmp_src_select.wires[JmpSrcSelect::Reg0 as usize];

        let use_offset_jmp = (jmp | je) | (jl | jg);

        let no_jmp_offset = no_jmp.expand() & Wires::<4>::parse_u8(1);
        let imm_offset = jmp_src_imm.expand() & input.imm;
        let reg_offset = jmp_src_reg.expand() & input.reg0;
        let target = no_jmp_offset | imm_offset | reg_offset;

        let flag_p_new = !input.reg0_write_data.wires[3] & !input.reg0_write_data.all_0();
        let flag_z_new = input.reg0_write_data.all_0();
        let flag_n_new = input.reg0_write_data.wires[3];
        let flag_p_next = mux2(input.flag_p, flag_p_new, input.reg0_write_enable);
        let flag_z_next = mux2(input.flag_z, flag_z_new, input.reg0_write_enable);
        let flag_n_next = mux2(input.flag_n, flag_n_new, input.reg0_write_enable);

        CpuBranchOutput {
            pc_offset_enable: no_jmp | use_offset_jmp,
            pc_offset: target,
            jmp_long_enable: jmp_long,
            jmp_long: target,
            flag_p: flag_p_next,
            flag_z: flag_z_next,
            flag_n: flag_n_next,
        }
    }
}
