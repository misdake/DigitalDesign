//! builtin library for the DSL: mul, mem, heap, Vec.
//!
//! each `define_*` is idempotent and declares its own dependencies, so users
//! only call the top-level define they need (no manual ordering, no double
//! registration). each library keeps its own tests in its own file.

pub mod arithmetic;
pub mod heap;
pub mod mem;
pub mod vec;

pub use arithmetic::*;
pub use heap::*;
pub use mem::*;
pub use vec::*;
