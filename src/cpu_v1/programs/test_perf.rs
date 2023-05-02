#![cfg(test)]

use crate::cpu_v1::isa::*;
use crate::cpu_v1::{CpuV1, CpuV1Instance, CpuV1State};
use crate::{clear_all, get_statistics, simulate};

#[test]
#[ignore]
fn raw_circuit() {
    clear_all();

    let mut inst_rom = [0u8; 256];
    let inst = &[
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
        inst_load_imm(0),       // r0 = 0
        inst_jmp_long(0),       // pc = 0
    ];
    inst.iter()
        .enumerate()
        .for_each(|(i, inst)| inst_rom[i] = inst.binary);

    let inst_rom = [0u8; 256];
    let mut state1 = CpuV1State::create(inst_rom);
    let _ = CpuV1Instance::build(&mut state1);

    // use crate::optimize;
    // optimize();

    let start = std::time::Instant::now();
    const CYCLES: usize = 10000;
    for _ in 0..CYCLES {
        simulate();
    }
    let duration = start.elapsed();
    println!("simulate {CYCLES} time: {}ms", duration.as_millis());
    println!(
        "{} cycles for 30fps",
        1. / 30. / (duration.as_secs_f64() / CYCLES as f64)
    );

    let result = get_statistics();
    println!("{:?}", result);
}
