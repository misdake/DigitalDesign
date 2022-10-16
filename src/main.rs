#![allow(incomplete_features)]
#![feature(generic_const_exprs)]

pub mod basic;
pub mod lib;
pub mod test;
pub mod wires;

pub use basic::*;
pub use lib::*;
pub use test::*;
pub use wires::*;

fn main() {
    // {
    //     // basic binary
    //     let a = input();
    //     let b = input();
    //
    //     let c = nand(a, b);
    //     let d = a & b;
    //     let e = a | b;
    //     let f = a ^ b;
    //
    //     test2_1("nand", a, b, c);
    //     test2_1("and", a, b, d);
    //     test2_1("or", a, b, e);
    //     test2_1("xor", a, b, f);
    // }

    // {
    //     // add_naive
    //     let a = &input_w::<8>();
    //     let b = &input_w::<8>();
    //     a.set_u8(123);
    //     b.set_u8(45);
    //     println!("a {:08b}", a.get_u8());
    //     println!("b {:08b}", b.get_u8());
    //     let c = a & b;
    //     let d = add_naive(a, b);
    //     execute_all_gates();
    //     println!("c {:08b}", c.get_u8());
    //     println!(
    //         "d {:08b}({}) {}",
    //         d.sum.get_u8(),
    //         d.sum.get_u8(),
    //         d.carry.get()
    //     );
    // }

    {
        // expand_signed
        let a = &input_w::<4>();
        let b = &a.expand_signed::<8>();
        a.set_u8(5);
        println!("a {:04b} b {:08b}", a.get_u8(), b.get_u8());
        a.set_u8(9);
        println!("a {:04b} b {:08b}", a.get_u8(), b.get_u8());
    }

    let r = execute_all_gates();
    println!("{:?}", r);
}
