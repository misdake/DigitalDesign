//! the rcc standard library, written in the rcc subset itself: these files
//! are real Rust modules (rust-analyzer/rustc read them) AND sources compiled
//! by `frontend::compile_program` into every program (unused fns are dropped
//! by the linker). `use crate::rcc_std::*;` exists for the IDE only.

#[allow(dead_code)]
pub mod heap;
#[allow(dead_code)]
pub mod mem;
#[allow(dead_code)]
pub mod mul;
#[allow(dead_code)]
pub mod vec;
