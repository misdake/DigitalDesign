mod register_operation1;

pub use register_operation1::*;

use std::fmt::{Debug, Formatter};

#[derive(Copy, Clone, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub struct Reg(pub u8); // u4 actually

impl Debug for Reg {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("r{}", self.0))
    }
}
