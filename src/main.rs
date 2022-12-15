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
    clear_all();

    let mut rom = Rom256x8::create();
    for i in 0..=255 {
        rom.set(i, 255 - i);
    }

    let addr = input_w::<8>();
    let _ = rom.apply(addr);

    let r = simulate();
    println!("{:?}", r);
}
