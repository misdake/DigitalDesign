// #![feature(generic_const_exprs)]
// #![allow(incomplete_features)]

pub mod basic_types;

pub use basic_types::*;

fn main() {
    let a = input();
    let b = input();

    let c = nand(a, b);

    a.set(false);
    b.set(false);
    execute_all_gates();
    println!("nand({},{}) => {}", a.get(), b.get(), c.get());

    a.set(true);
    b.set(false);
    execute_all_gates();
    println!("nand({},{}) => {}", a.get(), b.get(), c.get());

    a.set(false);
    b.set(true);
    execute_all_gates();
    println!("nand({},{}) => {}", a.get(), b.get(), c.get());

    a.set(true);
    b.set(true);
    execute_all_gates();
    println!("nand({},{}) => {}", a.get(), b.get(), c.get());
}
