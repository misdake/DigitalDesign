use crate::cpu_v1::isa::*;
use crate::cpu_v1::programs::{print_regs, test_cpu};

#[test]
fn test_load_store() {
    let state = test_cpu(
        &[
            inst_load_imm(15),  // r0 = 15
            inst_store_mem(15), // mem[15] = r0
            inst_load_imm(14),
            inst_store_mem(14),
            inst_load_imm(13), // r0 = 13
            inst_mov(0, 1),    // r1 = r0
            inst_store_mem(0), // mem[r1] = r0
            inst_load_mem(13),
            inst_mov(0, 3),
            inst_load_mem(14),
            inst_mov(0, 2),
            inst_load_imm(15),
            inst_mov(0, 1),
            inst_load_mem(0),
        ],
        15,
        print_regs,
    );

    assert_eq!(state.reg[0].out.get_u8(), 15);
    assert_eq!(state.reg[1].out.get_u8(), 15);
    assert_eq!(state.reg[2].out.get_u8(), 14);
    assert_eq!(state.reg[3].out.get_u8(), 13);
    assert_eq!(state.mem[13].out.get_u8(), 13);
    assert_eq!(state.mem[14].out.get_u8(), 14);
    assert_eq!(state.mem[15].out.get_u8(), 15);
}

#[test]
fn test_mem_page() {
    let state = test_cpu(
        &[
            inst_load_imm(15),
            inst_store_mem(15),  // mem[0][15] = 15
            inst_set_mem_page(), // mem[15]
            inst_load_mem(15),   // r0 = mem[15][15] (=0)
            inst_set_mem_page(), // mem[0]
            inst_load_mem(15),   // r0 = mem[0][15] (=15)
        ],
        8,
        print_regs,
    );

    assert_eq!(state.mem[15].out.get_u8(), 15);
    assert_eq!(state.reg[0].out.get_u8(), 15);
}
