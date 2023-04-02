use crate::cpu_v1::isa::*;
use crate::cpu_v1::programs::{print_regs, test_cpu};

#[test]
fn test_unary() {
    let _ = test_cpu(
        &[
            inst_inc(3),
            inst_inc(3),
            inst_inc(3),
            inst_inc(2),
            inst_inc(2),
            inst_inc(1),
            inst_inc(0),
            inst_neg(0),
            inst_inc(0),
        ],
        10,
        print_regs,
    );
}

#[test]
fn test_load_imm() {
    let state = test_cpu(
        &[
            inst_load_imm(3),
            inst_mov(0, 3),
            inst_load_imm(2),
            inst_mov(0, 2),
            inst_load_imm(1),
            inst_mov(0, 1),
            inst_load_imm(0),
        ],
        10,
        print_regs,
    );

    assert_eq!(state.reg[0].out.get_u8(), 0);
    assert_eq!(state.reg[1].out.get_u8(), 1);
    assert_eq!(state.reg[2].out.get_u8(), 2);
    assert_eq!(state.reg[3].out.get_u8(), 3);
}
