#![allow(clippy::manual_range_contains)]

mod assembler1;
mod assembler2;
mod register_allocator;
mod variable_allocator;

pub use assembler1::*;
pub use assembler2::*;
pub use register_allocator::*;
pub use variable_allocator::*;
