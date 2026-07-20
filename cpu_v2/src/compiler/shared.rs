//! shared definitions used by the assembler/linker and the driver.

use crate::sim::SimEnv;
use crate::{Instruction, SimState};

/// function name (static, registered in the compiler)
pub type FuncName = &'static str;

/// temporary register used by the linker for far calls and by codegen for
/// scratch sequences (never allocated)
pub const TMP_REG: u8 = 15;
pub const FUNCTION_TABLE_BASE: u16 = 0xff00;
pub const FUNCTION_TABLE_CAPACITY: usize = 256;

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
    /// 1 slot: call_abs through the initialized function-table entry
    CallAbs { index: u8 },
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

/// like `simulate` but without the per-instruction trace (for the rcc-run artifact)
pub fn simulate_quiet(instructions: &[Instruction], max_cycles: usize) -> (SimState, Option<u16>) {
    let mut sim = SimEnv::new(instructions);
    let halt_signal = sim.run_to_halt(max_cycles, |_, _, _| {});
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

/// decode an image written by `rcc` (magic "RCC1", count u32 LE, words u16 LE)
pub fn decode_binary(bytes: &[u8]) -> Option<Vec<Instruction>> {
    let (magic, rest) = bytes.split_at_checked(4)?;
    if magic != b"RCC1" {
        return None;
    }
    let (count_bytes, rest) = rest.split_at_checked(4)?;
    let count = u32::from_le_bytes(count_bytes.try_into().ok()?) as usize;
    if rest.len() != count * 2 {
        return None;
    }
    let mut out = Vec::with_capacity(count);
    for w in rest.chunks_exact(2) {
        let raw = u16::from_le_bytes([w[0], w[1]]);
        out.push(Instruction::parse(raw));
    }
    Some(out)
}
