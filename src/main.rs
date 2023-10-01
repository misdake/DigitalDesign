#![allow(incomplete_features)]
#![feature(generic_const_exprs)]

mod tests;

mod cpu_v1;

pub mod basic;
pub mod component_lib;
pub mod external;
pub mod reg;
pub mod wires;

pub use basic::*;
pub use component_lib::*;
pub use external::*;
pub use reg::*;
pub use wires::*;

mod export;

#[cfg(test)]
pub(crate) use tests::*;

pub(crate) fn select<T>(b: bool, t: T, f: T) -> T {
    if b {
        t
    } else {
        f
    }
}

fn main() {}
