//! rcc subset sample: function pointers + data pointers.
//! compiled by `compiler::frontend` tests; also valid host Rust.

use crate::dsl_rt::*;

fn double(x: u16) -> u16 {
    x + x
}

fn apply(f: fn(u16) -> u16, x: u16) -> u16 {
    f(x)
}

fn main() {
    let g: fn(u16) -> u16 = double;
    let r = apply(g, 21);
    let p = Ptr::from_addr(0x2000);
    p.write(0, r);
    let v = p.read(0);
    halt(v);
}
