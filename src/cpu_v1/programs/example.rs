use crate::cpu_v1::programs::*;
use crate::cpu_v1::*;

#[test]
fn test_fibonacci() {
    let _ = test_cpu(
        &[
            inst_load_imm(5),
            inst_mov(0, 3),
            inst_load_imm(1),
            inst_inc(1),
            inst_add(1, 0), // r0 = r0 + r1
            inst_mov(0, 2), // swap r0<>r1, save result to r2
            inst_mov(1, 0),
            inst_mov(2, 1),
            inst_dec(3),
            inst_jg_offset(16 - 5), // jump back to add
        ],
        35,
        print_regs,
    );
}

#[test]
fn test_fibonacci2() {
    use crate::cpu_v1::assembler::RegisterIndex::*;
    let mut asm = Assembler::new();
    asm.reg0().load_imm(5);
    asm.reg3().assign_from(Reg0);
    asm.reg0().load_imm(1);
    asm.reg1().inc();
    let add = asm.reg0().add_assign(Reg1);
    asm.reg2().assign_from(Reg0);
    asm.reg0().assign_from(Reg1);
    asm.reg1().assign_from(Reg2);
    asm.reg3().dec();
    asm.jg_offset(add);

    let _ = test_cpu(asm.finish().as_slice(), 35, print_regs);
}
