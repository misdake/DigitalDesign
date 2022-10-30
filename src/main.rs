#![allow(incomplete_features)]
#![feature(generic_const_exprs)]

pub mod basic;
pub mod lib;
pub mod reg;
pub mod test;
pub mod wires;

pub use basic::*;
pub use lib::*;
pub use reg::*;
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

    // {
    //     // expand_signed
    //     let a = &input_w::<4>();
    //     let b = &a.expand_signed::<8>();
    //     a.set_u8(5);
    //     println!("a {:04b} b {:08b}", a.get_u8(), b.get_u8());
    //     a.set_u8(9);
    //     println!("a {:04b} b {:08b}", a.get_u8(), b.get_u8());
    // }

    // {
    //     let a = input();
    //     let b = cycle(|b| a | b);
    //     let c = reg(a);
    //     let d = reg(c);
    //     for i in 0..20 {
    //         a.set(if i == 5 { 1 } else { 0 });
    //         simulate();
    //         println!("{} {} {} {}", a.get(), b.get(), c.get(), d.get());
    //     }
    // }
    // {
    //     let d = input();
    //     let e = input();
    //     let q = flipflop(d, e);
    //     for i in 0..20 {
    //         d.set(if i < 5 || i > 12 { 0 } else { 1 });
    //         e.set(if i == 9 || i == 15 { 1 } else { 0 });
    //         simulate();
    //         println!("{} {} {}", d.get(), e.get(), q.get());
    //     }
    // }

    {
        // add_naive
        let a = &input_w::<2>();
        let b = &input_w::<3>();
        let c = &input_w::<3>();
        let d = &input_w::<8>();
        a.set_u8(3);
        b.set_u8(0);
        c.set_u8(3);
        d.set_u8(46);
        println!("a {:08b}", a.get_u8());
        println!("b {:08b}", b.get_u8());
        println!("c {:08b}", c.get_u8());
        let f = &flatten3(a, b, c);
        let r = add_naive(d, f); // 3 + 3<<5 + 46 = 145
        simulate();
        println!("flatten");
        println!("f {:08b} width: {}", f.get_u8(), f.width());
        println!(
            "r {:08b}({}) {}",
            r.sum.get_u8(),
            r.sum.get_u8(),
            r.carry.get()
        );
        let (x, y, z) = unflatten3::<2, 3, 3>(&f);
        println!("unflatten");
        println!("a {:08b}", x.get_u8());
        println!("b {:08b}", y.get_u8());
        println!("c {:08b}", z.get_u8());
    }

    // let r = simulate();
    // println!("{:?}", r);
}
