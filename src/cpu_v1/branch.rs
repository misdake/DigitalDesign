use crate::cpu_v1::{CpuComponent, CpuComponentEmu};
use crate::{input, input_w, Wire, Wires};

#[derive(Clone)]
pub struct CpuBranchInput {
    pub imm: Wires<4>,
    pub alu_out: Wires<4>,
    pub flag_write_enable: Wire,

    pub branch_op: Wires<6>,

    pub no_jmp_enable: Wire,
    pub jmp_offset_enable: Wire,
    pub jmp_offset: Wires<4>,
    pub jmp_long_enable: Wire,
    pub jmp_long: Wires<4>,
}
#[derive(Clone)]
pub struct CpuBranchOutput {
    pub no_jmp_enable: Wire,
    pub jmp_offset_enable: Wire,
    pub jmp_offset: Wires<4>,
    pub jmp_long_enable: Wire,
    pub jmp_long: Wires<4>,
}

pub struct CpuBranch;
impl CpuComponent for CpuBranch {
    type Input = CpuBranchInput;
    type Output = CpuBranchOutput;
    fn build(input: &Self::Input) -> Self::Output {
        todo!()
    }
}
pub struct CpuBranchEmu;
impl CpuComponentEmu<CpuBranch> for CpuBranchEmu {
    fn init_output() -> CpuBranchOutput {
        CpuBranchOutput {
            no_jmp_enable: input(),
            jmp_offset_enable: input(),
            jmp_offset: input_w(),
            jmp_long_enable: input(),
            jmp_long: input_w(),
        }
    }

    fn execute(input: &CpuBranchInput, output: &CpuBranchOutput) {
        todo!()
    }
}
