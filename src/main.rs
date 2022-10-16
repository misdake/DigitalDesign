// #![feature(generic_const_exprs)]
// #![allow(incomplete_features)]

pub mod basic;
pub mod binary_logic;
pub mod test;

pub use basic::*;
pub use binary_logic::*;
pub use test::*;

fn main() {
    let a = input();
    let b = input();

    let c = nand(a, b);
    let d = a & b;
    let e = a | b;
    let f = a ^ b;

    test2("nand", a, b, c);
    test2("and", a, b, d);
    test2("or", a, b, e);
    test2("xor", a, b, f);

    let r = execute_all_gates();
    println!("{:?}", r);
}
