//! builtin library ported to the new DSL (M4): mul, mem, heap, Vec.
//!
//! each `define_*` is idempotent and declares its own dependencies, so users
//! only call the top-level define they need (no manual ordering, no double
//! registration).

pub mod arithmetic;
pub mod heap;
pub mod mem;
pub mod vec;

pub use arithmetic::*;
pub use heap::*;
pub use mem::*;
pub use vec::*;
