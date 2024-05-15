#![allow(clippy::manual_range_contains)]

mod assembler1;
mod assembler2;
mod op;
mod register_allocator;
mod variable;
mod variable_allocator;
mod variable_operation1;
mod variable_operation2;
mod variable_operation3;

pub use assembler1::*;
pub use assembler2::*;
pub use op::*;
pub use register_allocator::*;
pub use variable::*;
pub use variable_allocator::*;
pub use variable_operation1::*;
pub use variable_operation2::*;
