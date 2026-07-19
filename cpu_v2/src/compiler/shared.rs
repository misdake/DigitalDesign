//! shared definitions used by the assembler/linker and the driver.

use crate::sim::SimEnv;
use crate::{Instruction, SimState};

/// function name (static, registered in the compiler)
pub type FuncName = &'static str;

/// temporary register used by the linker for far calls and by codegen for
/// scratch sequences (never allocated)
pub const TMP_REG: u8 = 15;

#[derive(Clone, Debug)]
pub struct FuncDecl {
    pub func_name: FuncName,
    pub param_names: Vec<&'static str>,
    pub return_value_names: Vec<&'static str>,
}
impl FuncDecl {
    pub fn new(
        func_name: FuncName,
        param_names: &[&'static str],
        return_value_names: &[&'static str],
    ) -> Self {
        Self {
            func_name,
            param_names: param_names.to_vec(),
            return_value_names: return_value_names.to_vec(),
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub enum RelocKind {
    /// 3 slots: near -> call_rel + 2 nop; far -> load_lo(tmp) + load_hi(tmp) + call_reg(tmp)
    Call3,
    /// 2 slots: load_lo(reg) + load_hi(reg), filling in the absolute function address
    LoadAddr { reg: u8 },
}

#[derive(Clone, Debug)]
pub struct Relocation {
    pub func_name: FuncName,
    pub kind: RelocKind,
    pub slots: Vec<crate::compiler::InstructionSlot>,
}

pub fn u8_to_hi_lo(v: u8) -> (u8, u8) {
    (v >> 4, v & 0xf)
}

/// run a compiled program on the simulator with a cycle cap (tests must
/// always pass a cap so a runaway program cannot hang the test suite)
pub fn simulate(instructions: &[Instruction], max_cycles: usize) -> (SimState, Option<u16>) {
    let mut sim = SimEnv::new(instructions);
    let halt_signal = sim.run_to_halt(max_cycles, |pc, inst, change| {
        let inst = format!("pc {pc:04x}: {inst}");
        let change = change.desc(pc);
        println!("{inst:40}{change}");
    });
    if let Some(halt_signal) = halt_signal {
        println!(
            "halt with signal = {halt_signal} after {} cycles",
            sim.state.cycles
        )
    }
    (sim.state, halt_signal)
}

/// flat disassembly of an instruction slice (no function boundaries; for a
/// per-function listing see `Compiler::finish`)
pub fn disassemble(instructions: &[Instruction]) -> String {
    instructions
        .iter()
        .enumerate()
        .map(|(addr, inst)| format!("{addr:04x}: {inst}"))
        .collect::<Vec<_>>()
        .join("\n")
}
