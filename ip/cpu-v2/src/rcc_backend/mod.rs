//! compiler pipeline (see docs/compiler_redesign.md): Rust-embedded DSL ->
//! SSA/CFG IR -> optimization passes -> linear-scan register allocation ->
//! codegen, whole-program layout, and final encoding.

mod assembler;
mod codegen;
mod driver;
mod linker;
mod options;
mod shared;

#[cfg(test)]
mod tests;

pub use assembler::*;
pub use codegen::*;
pub use driver::*;
pub use options::*;
pub use rcc::*;
pub use shared::*;

pub const RET_REGS: [u8; 2] = [0, 1];
pub const ARG_REGS: [u8; 6] = [2, 3, 4, 5, 6, 7];
pub const ALLOCATABLE_REGS: [u8; 13] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];
pub const CALLER_SAVED: [u8; 8] = [0, 1, 2, 3, 4, 5, 6, 7];
pub const CALLEE_SAVED: [u8; 5] = [8, 9, 10, 11, 12];
pub const REG_RA: u8 = 13;
pub const REG_SP: u8 = 14;
pub const REG_TMP: u8 = 15;

pub const V2_REGISTER_CONVENTION: rcc::RegisterConvention = rcc::RegisterConvention {
    return_registers: &RET_REGS,
    argument_registers: &ARG_REGS,
    allocatable_registers: &ALLOCATABLE_REGS,
    caller_saved: &CALLER_SAVED,
    callee_saved: &CALLEE_SAVED,
    link_register: REG_RA,
    stack_register: REG_SP,
    temporary_register: REG_TMP,
    maximum_frame_words: 255,
};
