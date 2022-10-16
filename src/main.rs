// #![feature(generic_const_exprs)]
// #![allow(incomplete_features)]

pub mod basic_types;

pub use basic_types::*;

fn main() {
    let a = input();
    let b = input();

    let c = nand(a, b);

    a.set(0);
    b.set(0);
    execute_all_gates();
    println!("nand({},{}) => {}", a.get(), b.get(), c.get());

    a.set(1);
    b.set(0);
    execute_all_gates();
    println!("nand({},{}) => {}", a.get(), b.get(), c.get());

    a.set(0);
    b.set(1);
    execute_all_gates();
    println!("nand({},{}) => {}", a.get(), b.get(), c.get());

    a.set(1);
    b.set(1);
    let result = execute_all_gates();
    println!("nand({},{}) => {}", a.get(), b.get(), c.get());
    println!("{:?}", result);
}
