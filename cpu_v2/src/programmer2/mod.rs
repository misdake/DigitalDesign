//! new compiler pipeline (see docs/compiler_redesign.md): DSL frontend ->
//! SSA/CFG IR -> liveness -> linear-scan register allocation -> codegen.
//! coexists with the legacy `programmer` module until migration completes (M6).

mod builder;
mod codegen;
mod compiler2;
mod ir;
mod regalloc;

pub use builder::*;
pub use codegen::*;
pub use compiler2::*;
pub use ir::*;
pub use regalloc::*;
