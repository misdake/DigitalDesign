#![allow(incomplete_features)]
#![feature(generic_const_exprs)]

mod tests;

mod cpu_v1;

pub mod basic;
pub mod external;
pub mod lib;
pub mod reg;
pub mod wires;

pub use basic::*;
pub use external::*;
pub use lib::*;
pub use reg::*;
pub use wires::*;

use crate::cpu_v1::inst_mov;
#[cfg(test)]
pub(crate) use tests::*;

pub(crate) fn select<T>(b: bool, t: T, f: T) -> T {
    if b {
        t
    } else {
        f
    }
}

fn main() {
    let inst: u8 = 0b0001_01_11;
    let inst2 = inst_mov(0b01, 0b11).binary;
    cpu_v1::InstDesc::parse(inst);
    cpu_v1::InstDesc::parse(inst2);
}
