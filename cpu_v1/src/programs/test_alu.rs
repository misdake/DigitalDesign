use crate::isa::Instruction::*;
use crate::isa::RegisterIndex::*;
use crate::programs::{print_regs, test_cpu_with_emu};
use digital_design_code::global_lock;

#[test]
fn test_unary() {
    let _lock = global_lock();
    test_cpu_with_emu(
        &[
            inc(Reg3),
            inc(Reg3),
            inv(Reg3),
            inc(Reg2),
            inc(Reg2),
            inc(Reg1),
            inc(Reg0),
            neg(Reg0),
            inc(Reg0),
        ],
        10,
        print_regs,
    );
}

#[test]
fn test_binary() {
    let _lock = global_lock();
    test_cpu_with_emu(
        &[
            load_imm(2),
            mov((Reg0, Reg3)),
            load_imm(7),
            mov((Reg0, Reg2)),
            and((Reg3, Reg2)),
            mov((Reg0, Reg2)),
            xor((Reg3, Reg2)),
            mov((Reg0, Reg2)),
            or((Reg3, Reg2)),
            mov((Reg0, Reg2)),
            add((Reg3, Reg2)),
        ],
        13,
        print_regs,
    );
}

#[test]
fn test_load_imm() {
    let _lock = global_lock();
    test_cpu_with_emu(
        &[
            load_imm(3),
            mov((Reg0, Reg3)),
            load_imm(2),
            mov((Reg0, Reg2)),
            load_imm(1),
            mov((Reg0, Reg1)),
            load_imm(0),
        ],
        10,
        print_regs,
    );
}
