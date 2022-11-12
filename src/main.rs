#![allow(incomplete_features)]
#![feature(generic_const_exprs)]

mod tests;

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

fn main() {

    // let r = simulate();
    // println!("{:?}", r);
}
