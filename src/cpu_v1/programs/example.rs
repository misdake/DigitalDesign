use crate::cpu_v1::isa::*;
use crate::cpu_v1::programs::{print_regs, test_cpu};

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
