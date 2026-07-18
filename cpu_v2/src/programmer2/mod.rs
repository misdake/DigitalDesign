//! new compiler pipeline (see docs/compiler_redesign.md): DSL frontend ->
//! SSA/CFG IR -> liveness -> linear-scan register allocation -> codegen.
//! coexists with the legacy `programmer` module until migration completes (M6).

mod builder;
mod ir;

pub use builder::*;
pub use ir::*;
