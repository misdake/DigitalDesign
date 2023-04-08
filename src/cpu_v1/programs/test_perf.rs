#![cfg(test)]

use crate::cpu_v1::{CpuV1, CpuV1Instance, CpuV1State};
use crate::{clear_all, execute_gates};

#[test]
fn raw_circuit() {
    clear_all();

    let inst_rom = [0u8; 256];
    let mut state1 = CpuV1State::create(inst_rom);
    let _ = CpuV1Instance::build(&mut state1);

    let result = execute_gates();
    println!("{:?}", result);
}
