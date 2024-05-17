mod register_allocator;
mod register_operation1;

pub use register_allocator::*;
pub use register_operation1::*;

pub type Reg = u8; // u4 actually
