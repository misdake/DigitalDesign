use crate::cpu_v1::isa::*;
use crate::cpu_v1::programs::{print_regs, test_cpu};

#[test]
fn test_jmp() {
    let state = test_cpu(
        &[
            inst_jmp_offset(2),      // 0  0000
            inst_load_imm(2),        // 1  0001
            inst_jmp_offset(3),      // 2  0010
            inst_inc(2),             // 3  0011
            inst_jmp_offset(2),      // 4  0100
            inst_jmp_offset(16 - 2), // 5  0101
            inst_jmp_offset(0),      // 6  0110
        ],
        10,
        print_regs,
    );

    // 0 0000 -> 2 0010 -> 5 0101 -> 3 0011 -> 4 0100 -> 6 0110

    assert_eq!(state.reg[0].out.get_u8(), 0);
    assert_eq!(state.reg[1].out.get_u8(), 0);
    assert_eq!(state.reg[2].out.get_u8(), 1);
    assert_eq!(state.reg[3].out.get_u8(), 0);
}

#[test]
fn test_jmp_condition_taken() {
    let state = test_cpu(
        &[
            inst_load_imm(1),       //  0  0000
            inst_jne_offset(3),     //  1  0001
            inst_load_imm(8),       //  2  0010
            inst_jl_offset(3),      //  3  0011
            inst_load_imm(1),       //  4  0100
            inst_jg_offset(16 - 3), //  5  0101
        ],
        7,
        print_regs,
    );

    // 0 0000 -> 1 0001 -> 4 0100 -> 5 0101 -> 2 0010 -> 3 0011 -> 6 idle

    assert_eq!(state.reg[0].out.get_u8(), 8);
}

#[test]
fn test_jmp_condition_not_taken() {
    let state = test_cpu(
        &[
            inst_load_imm(0),   //  0  0000
            inst_jne_offset(3), //  1  0001
            inst_load_imm(1),   //  2  0010
            inst_jl_offset(7),  //  3  0011
            inst_load_imm(8),   //  4  0100
            inst_jg_offset(7),  //  5  0101
            inst_load_imm(13),  //  6  0110
        ],
        8,
        print_regs,
    );

    assert_eq!(state.reg[0].out.get_u8(), 13);
}

#[test]
fn test_jmp_condition_reg() {
    let state = test_cpu(
        &[
            inst_load_imm(2), // 0 or 1 or 2 or 3
            inst_add(0, 0),
            inst_inc(0), // 2x+1
            inst_jmp_offset(0),
            inst_load_imm(10), // 0 jmp to here
            inst_jmp_offset(7),
            inst_load_imm(11), // 1 jmp to here
            inst_jmp_offset(5),
            inst_load_imm(12), // 2 jmp to here
            inst_jmp_offset(3),
            inst_load_imm(13), // 3 jmp to here
            inst_jmp_offset(1),
        ],
        6,
        print_regs,
    );

    assert_eq!(state.reg[0].out.get_u8(), 12);
}

#[test]
fn test_jmp_long() {
    let mut inst_rom = [inst_mov(0, 0); 256];
    inst_rom[0] = inst_jmp_long(1); // -> 16
    inst_rom[16] = inst_jmp_long(4); // -> 64
    inst_rom[32] = inst_jmp_long(5); // -> 80
    inst_rom[48] = inst_jmp_long(2); // -> 32
    inst_rom[64] = inst_load_imm(2); // -> 65
    inst_rom[65] = inst_jmp_long(3); // -> 48
    inst_rom[80] = inst_load_imm(15);
    let state = test_cpu(inst_rom.as_slice(), 8, print_regs);

    assert_eq!(state.reg[0].out.get_u8(), 15);
}

#[test]
fn test_loop() {
    let state = test_cpu(
        &[
            inst_load_imm(7),
            inst_inc(1), // r += 1
            inst_dec(0), // i -= 1
            inst_jg_offset(16 - 2),
        ],
        25,
        print_regs,
    );

    assert_eq!(state.reg[0].out.get_u8(), 0);
    assert_eq!(state.reg[1].out.get_u8(), 7);
    assert!(state.pc.out.get_u8() >= 4);
}
