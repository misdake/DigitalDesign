use crate::cpu_v1::programs::*;
use crate::cpu_v1::*;
use crate::global_lock;

#[test]
fn test_fibonacci() {
    let _lock = global_lock();
    use isa::Instruction::*;
    use isa::RegisterIndex::*;
    test_cpu_with_emu(
        &[
            load_imm(5),
            mov((Reg0, Reg3)),
            load_imm(1),
            inc(Reg1),
            add((Reg1, Reg0)), // r0 = r0 + r1
            mov((Reg0, Reg2)), // swap r0<>r1, save result to r2
            mov((Reg1, Reg0)),
            mov((Reg2, Reg1)),
            dec(Reg3),
            jg_offset(16 - 5), // jump back to add
        ],
        35,
        print_regs,
    );
}

#[test]
fn test_fibonacci2() {
    let _lock = global_lock();
    use isa::RegisterIndex::*;
    let mut asm = Assembler::new();
    asm.reg0().load_imm(5);
    asm.reg3().assign_from(Reg0);
    asm.reg0().load_imm(1);
    asm.reg1().inc();
    let loop_start = asm.reg0().add_assign(Reg1);
    asm.reg2().assign_from(Reg0);
    asm.reg0().assign_from(Reg1);
    asm.reg1().assign_from(Reg2);
    asm.reg3().dec();
    asm.jg_back(loop_start);

    test_cpu_with_emu(asm.finish().as_slice(), 35, print_regs);
}
