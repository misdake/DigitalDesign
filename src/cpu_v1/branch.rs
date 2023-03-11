use crate::cpu_v1::CpuComponent;
use crate::{Wire, Wires};

#[derive(Clone)]
pub struct CpuBranchInput {
    pub imm: Wires<4>,
    pub reg0: Wires<4>,
    pub alu_out: Wires<4>,
    pub flag_write_enable: Wire,

    pub jmp_op: Wires<7>, // JmpOp: no_jmp, jmp, je, jl, jg, reg, long

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
        let no_jmp = input.jmp_op.wires[0]
            | (input.jmp_op.wires[2] & !input.flag_z)
            | (input.jmp_op.wires[3] & !input.flag_n)
            | (input.jmp_op.wires[4] & !input.flag_p);

        let jmp = input.jmp_op.wires[1];
        let je = input.jmp_op.wires[2] & input.flag_z;
        let jl = input.jmp_op.wires[3] & input.flag_n;
        let jg = input.jmp_op.wires[4] & input.flag_p;

        let jmp_reg = input.jmp_op.wires[5];
        let jmp_long = input.jmp_op.wires[6];

        let use_offset_jmp = (jmp | je) | (jl | jg);
        let use_offset = (no_jmp | jmp_reg) | use_offset_jmp;
        let no_jmp_offset = no_jmp.expand() & Wires::<4>::parse_u8(1);
        let imm_offset = use_offset.expand() & input.imm;
        let reg_offset = jmp_reg.expand() & input.reg0;
        let offset = no_jmp_offset | imm_offset | reg_offset;

        CpuBranchOutput {
            pc_offset_enable: use_offset,
            pc_offset: offset,
            jmp_long_enable: jmp_long,
            jmp_long: input.imm,
            flag_p: input.alu_out.wires[3],
            flag_z: input.alu_out.all_0(),
            flag_n: !input.alu_out.wires[3],
        }
    }
}
