use crate::cpu_v1::isa::Instruction::*;
use crate::cpu_v1::isa::RegisterIndex::*;
use crate::cpu_v1::programs::{print_regs, test_cpu};

#[test]
fn test_load_store() {
    let state = test_cpu(
        &[
            load_imm(15),  // r0 = 15
            store_mem(15), // mem[15] = r0
            load_imm(14),
            store_mem(14),
            load_imm(13),      // r0 = 13
            mov((Reg0, Reg1)), // r1 = r0
            store_mem(0),      // mem[r1] = r0
            load_mem(13),
            mov((Reg0, Reg3)),
            load_mem(14),
            mov((Reg0, Reg2)),
            load_imm(15),
            mov((Reg0, Reg1)),
            load_mem(0),
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
            load_imm(15),
            store_mem(15),    // mem[0][15] = 15
            set_mem_page(()), // mem[15]
            load_mem(15),     // r0 = mem[15][15] (=0)
            set_mem_page(()), // mem[0]
            load_mem(15),     // r0 = mem[0][15] (=15)
        ],
        8,
        print_regs,
    );

    assert_eq!(state.mem[15].out.get_u8(), 15);
    assert_eq!(state.reg[0].out.get_u8(), 15);
}
