use crate::isa::Instruction;
use crate::isa::Instruction::*;
use crate::isa::RegisterIndex::*;
use crate::programs::{print_regs, test_cpu_with_emu};
use digital_design_code::global_lock;

#[test]
fn test_jmp() {
    let _lock = global_lock();
    test_cpu_with_emu(
        &[
            jmp_offset(2),      // 0  0000
            load_imm(2),        // 1  0001
            jmp_offset(3),      // 2  0010
            inc(Reg2),          // 3  0011
            jmp_offset(2),      // 4  0100
            jmp_offset(16 - 2), // 5  0101
            jmp_offset(0),      // 6  0110
        ],
        10,
        print_regs,
    );

    // 0 0000 -> 2 0010 -> 5 0101 -> 3 0011 -> 4 0100 -> 6 0110
}

#[test]
fn test_jmp_condition_taken() {
    let _lock = global_lock();
    test_cpu_with_emu(
        &[
            load_imm(1),       //  0  0000
            jne_offset(3),     //  1  0001
            load_imm(8),       //  2  0010
            jl_offset(3),      //  3  0011
            load_imm(1),       //  4  0100
            jg_offset(16 - 3), //  5  0101
        ],
        7,
        print_regs,
    );

    // 0 0000 -> 1 0001 -> 4 0100 -> 5 0101 -> 2 0010 -> 3 0011 -> 6 idle
}

#[test]
fn test_jmp_condition_not_taken() {
    let _lock = global_lock();
    test_cpu_with_emu(
        &[
            load_imm(0),   //  0  0000
            jne_offset(3), //  1  0001
            load_imm(1),   //  2  0010
            jl_offset(7),  //  3  0011
            load_imm(8),   //  4  0100
            jg_offset(7),  //  5  0101
            load_imm(13),  //  6  0110
        ],
        8,
        print_regs,
    );
}

#[test]
fn test_jmp_condition_reg() {
    let _lock = global_lock();
    test_cpu_with_emu(
        &[
            load_imm(2), // 0 or 1 or 2 or 3
            add((Reg0, Reg0)),
            inc(Reg0), // 2x+1
            jmp_offset(0),
            load_imm(10), // 0 jmp to here
            jmp_offset(7),
            load_imm(11), // 1 jmp to here
            jmp_offset(5),
            load_imm(12), // 2 jmp to here
            jmp_offset(3),
            load_imm(13), // 3 jmp to here
            jmp_offset(1),
        ],
        6,
        print_regs,
    );
}

#[test]
fn test_jmp_long() {
    let _lock = global_lock();
    let mut inst_rom = [Instruction::default(); 256];
    inst_rom[0] = jmp_long(1); // -> 16
    inst_rom[16] = jmp_long(4); // -> 64
    inst_rom[32] = jmp_long(5); // -> 80
    inst_rom[48] = jmp_long(2); // -> 32
    inst_rom[64] = load_imm(2); // -> 65
    inst_rom[65] = jmp_long(3); // -> 48
    inst_rom[80] = load_imm(15);
    test_cpu_with_emu(inst_rom.as_slice(), 8, print_regs);
}

#[test]
fn test_loop() {
    let _lock = global_lock();
    test_cpu_with_emu(
        &[
            load_imm(7),
            inc(Reg1), // r += 1
            dec(Reg0), // i -= 1
            jg_offset(16 - 2),
        ],
        25,
        print_regs,
    );
}

#[test]
fn test_loop2() {
    let _lock = global_lock();
    test_cpu_with_emu(
        &[
            load_imm(2),
            mov((Reg0, Reg1)),
            dec(Reg1),
            load_imm(2),
            inc(Reg3),          // log loop count
            dec(Reg0),          // inner-=1
            jne_offset(16 - 2), // inner loop done
            mov((Reg1, Reg1)),
            jne_offset(16 - 6),
        ],
        25,
        print_regs,
    );
}
