#![allow(incomplete_features)]
#![feature(generic_const_exprs)]

pub mod arithmetic;
pub mod basic;
pub mod binary_logic;
pub mod test;
pub mod wires;

pub use arithmetic::*;
pub use basic::*;
pub use binary_logic::*;
pub use test::*;
pub use wires::*;
pub use wires::*;

fn main() {
    // let a = input();
    // let b = input();
    //
    // let c = nand(a, b);
    // let d = a & b;
    // let e = a | b;
    // let f = a ^ b;
    //
    // test2_1("nand", a, b, c);
    // test2_1("and", a, b, d);
    // test2_1("or", a, b, e);
    // test2_1("xor", a, b, f);

    {
        let a = &input_w::<8>();
        let b = &input_w::<8>();
        a.set_u8(123);
        b.set_u8(45);
        println!("a {:08b}", a.get_u8());
        println!("b {:08b}", b.get_u8());
        let c = a & b;
        let d = add_naive(a, b);
        execute_all_gates();
        println!("c {:08b}", c.get_u8());
        println!(
            "d {:08b}({}) {}",
            d.sum.get_u8(),
            d.sum.get_u8(),
            d.carry.get()
        );
    }

    // {
    //     let a = input();
    //     let b = input();
    //     let c = input();
    //     let r = add(a, b, c);
    //     test3_2("add", a, b, c, r.sum, r.carry);
    // }

    let r = execute_all_gates();
    println!("{:?}", r);
}
