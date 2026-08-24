#![allow(dead_code)]
//! shared helpers for the integration tests

use crate::{Compiler, FuncBuilder, Instruction, IrFunc, SimState, VReg};

/// compile the given functions and run `main` on the simulator.
/// `max_cycles` is mandatory so a runaway program cannot hang the suite.
pub fn compile_and_run(
    funcs: Vec<IrFunc>,
    main: &'static str,
    max_cycles: usize,
) -> (SimState, Option<u16>) {
    let mut c = Compiler::new();
    for f in funcs {
        c.add_func(f);
    }
    let (instructions, _) = c.finish(main);
    crate::simulate(&instructions, max_cycles)
}

/// compile with a custom compiler (e.g. optimization flags), and run
pub fn compile_with_and_run(
    mut compiler: Compiler,
    funcs: Vec<IrFunc>,
    main: &'static str,
    max_cycles: usize,
) -> (SimState, Option<u16>) {
    for f in funcs {
        compiler.add_func(f);
    }
    let (instructions, _) = compiler.finish(main);
    crate::simulate(&instructions, max_cycles)
}

pub fn imm_seq(b: &mut FuncBuilder, values: &[u16]) -> Vec<VReg> {
    values.iter().map(|&v| b.load_imm(v)).collect()
}

/// all sp_sub immediate values in the program (stack frame sizes)
pub fn sp_sub_values(instructions: &[Instruction]) -> Vec<u16> {
    instructions
        .iter()
        .filter_map(|i| match i {
            Instruction::sp_sub(hi, lo) => Some(((*hi as u16) << 4) | *lo as u16),
            _ => None,
        })
        .collect()
}

/// all sp_add immediate values in the program (frame teardown sizes)
pub fn sp_add_values(instructions: &[Instruction]) -> Vec<u16> {
    instructions
        .iter()
        .filter_map(|i| match i {
            Instruction::sp_add(hi, lo) => Some(((*hi as u16) << 4) | *lo as u16),
            _ => None,
        })
        .collect()
}
