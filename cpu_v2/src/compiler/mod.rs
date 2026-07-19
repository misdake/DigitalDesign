//! compiler pipeline (see docs/compiler_redesign.md): Rust-embedded DSL ->
//! SSA/CFG IR -> optimization passes -> linear-scan register allocation ->
//! codegen. reuses the Assembler/Linker from the previous implementation.

mod assembler;
mod builder;
mod codegen;
mod driver;
mod ir;
mod linker;
mod passes;
mod regalloc;
mod shared;

pub mod dsl;

#[cfg(test)]
mod tests;

pub use assembler::*;
pub use builder::*;
pub use codegen::*;
pub use driver::*;
pub use ir::*;
pub use linker::*;
pub use passes::*;
pub use regalloc::*;
pub use shared::*;
