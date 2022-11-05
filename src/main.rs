#![allow(incomplete_features)]
#![feature(generic_const_exprs)]

mod tests;

pub mod basic;
pub mod lib;
pub mod reg;
pub mod wires;

pub use basic::*;
pub use lib::*;
pub use reg::*;
pub use wires::*;

fn main() {

    // {
    //     // flatten & unflatten
    //     let a = &input_w::<2>();
    //     let b = &input_w::<3>();
    //     let c = &input_w::<3>();
    //     let d = &input_w::<8>();
    //     a.set_u8(3);
    //     b.set_u8(0);
    //     c.set_u8(3);
    //     d.set_u8(46);
    //     println!("a {:08b}", a.get_u8());
    //     println!("b {:08b}", b.get_u8());
    //     println!("c {:08b}", c.get_u8());
    //     let f = &flatten3(a, b, c);
    //     let r = add_naive(d, f); // 3 + 3<<5 + 46 = 145
    //     simulate();
    //     println!("flatten");
    //     println!("f {:08b} width: {}", f.get_u8(), f.width());
    //     println!(
    //         "r {:08b}({}) {}",
    //         r.sum.get_u8(),
    //         r.sum.get_u8(),
    //         r.carry.get()
    //     );
    //     let (x, y, z) = unflatten3::<2, 3, 3>(&f);
    //     println!("unflatten");
    //     println!("a {:08b}", x.get_u8());
    //     println!("b {:08b}", y.get_u8());
    //     println!("c {:08b}", z.get_u8());
    // }

    // let r = simulate();
    // println!("{:?}", r);
}
