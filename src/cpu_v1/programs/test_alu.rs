use crate::cpu_v1::isa::Instruction::*;
use crate::cpu_v1::isa::RegisterIndex::*;
use crate::cpu_v1::programs::{print_regs, test_cpu};

#[test]
fn test_unary() {
    let _ = test_cpu(
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
    let _ = test_cpu(
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
    let state = test_cpu(
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

    assert_eq!(state.reg[0].out.get_u8(), 0);
    assert_eq!(state.reg[1].out.get_u8(), 1);
    assert_eq!(state.reg[2].out.get_u8(), 2);
    assert_eq!(state.reg[3].out.get_u8(), 3);
}
