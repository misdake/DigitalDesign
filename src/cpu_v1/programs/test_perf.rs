#![cfg(test)]

use crate::cpu_v1::isa::Instruction;
use crate::cpu_v1::isa::Instruction::*;
use crate::cpu_v1::isa::RegisterIndex::*;
use crate::cpu_v1::{CpuV1, CpuV1MixInstance, CpuV1State};
use crate::{clear_all, get_statistics, simulate};

#[test]
#[ignore]
fn raw_circuit() {
    clear_all();

    let mut inst_rom = [Instruction::default(); 256];
    let inst = &[
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
        load_imm(0),       // r0 = 0
        jmp_long(0),       // pc = 0
    ];
    inst.iter()
        .enumerate()
        .for_each(|(i, inst)| inst_rom[i] = *inst);

    let mut state1 = CpuV1State::create(inst_rom);
    let _ = CpuV1MixInstance::build(&mut state1);

    let start = std::time::Instant::now();
    const CYCLES: usize = 100000;
    for _ in 0..CYCLES {
        simulate();
    }
    let duration = start.elapsed();
    println!("simulate {CYCLES} cycles: {}ms", duration.as_millis());
    let time_per_cycle = duration.as_secs_f64() / CYCLES as f64;
    println!("{:.0} cycles for 30fps", 1. / 30. / time_per_cycle);
    println!("{:.0} cycles for 60fps", 1. / 60. / time_per_cycle);

    let result = get_statistics();
    println!("{:?}", result);
}
