#![allow(incomplete_features)]
#![feature(generic_const_exprs)]

pub use cpu_v1::*;

pub mod devices;

use digital_design_circuit::Circuit;

pub fn build_cpu_v1_sim(
    instructions: [Instruction; 256],
) -> (Circuit, CpuV1State, CpuV1StateInternal) {
    cpu_v1_build_mix_with_bus(instructions, shared_device_bus(devices::Devices::new()))
}

#[cfg(test)]
mod programs;
