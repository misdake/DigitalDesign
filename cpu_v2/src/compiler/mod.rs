//! compiler pipeline (see docs/compiler_redesign.md): Rust-embedded DSL ->
//! SSA/CFG IR -> optimization passes -> linear-scan register allocation ->
//! codegen, whole-program layout, and final encoding.

mod assembler;
mod builder;
mod codegen;
mod debug;
mod driver;
mod g16;
mod ir;
mod linker;
mod options;
mod passes;
mod regalloc;
mod shared;

#[cfg(test)]
mod tests;

pub use assembler::*;
pub use builder::*;
pub use codegen::*;
pub use debug::*;
pub use driver::*;
pub use g16::*;
pub use ir::*;
pub use options::*;
pub use passes::*;
pub use regalloc::*;
pub use shared::*;
