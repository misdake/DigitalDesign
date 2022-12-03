use crate::{decode2, input, mux2_w, reduce4, reg_w, Wire, Wires};

pub trait Regfile<const ADDR: usize, const WIDTH: usize, const READ: usize, const WRITE: usize> {
    fn apply(
        addr: [Wires<ADDR>; READ],
        write_enable: Wires<WRITE>,
        write_data: [Wires<WIDTH>; WRITE],
        reset_all: Wire,
    ) -> [Wires<WIDTH>; READ];
}

pub struct Regfile4x4_1R1W;
impl Regfile<2, 4, 1, 1> for Regfile4x4_1R1W {
    fn apply(
        addr: [Wires<2>; 1],
        write_enable: Wires<1>,
        write_data: [Wires<4>; 1],
        reset_all: Wire,
    ) -> [Wires<4>; 1] {
        let port0_enable_each = decode2(addr[0]);

        let mut reg = [reg_w::<4>(), reg_w::<4>(), reg_w::<4>(), reg_w::<4>()];

        let mut port0_read_each: [Wires<4>; 4] = [Wires {
            wires: [Wire(0); 4],
        }; 4];

        let zero = input();
        zero.set(0);

        for i in 0..4 {
            let enable = port0_enable_each[i];
            port0_read_each[i] = enable.expand() & reg[i].out;

            let port0_write_data0 = mux2_w(reg[i].out, zero.expand(), reset_all);
            let port0_write_data1 = write_data[0];
            let port0_write_enable = enable & write_enable.wires[0];
            let port0_write_data = mux2_w(port0_write_data0, port0_write_data1, port0_write_enable);
            reg[i].set_in(port0_write_data)
        }

        let port0_read = reduce4(port0_read_each.as_slice(), &|a, b| a | b);
        [port0_read]
    }
}
#[test]
fn test_regfile4x4_1r1w() {
    use crate::*;
    clear_all();

    let reset_all = input();
    let addr = [input_w::<2>()];
    let write_data = [input_w::<4>()];
    let write_enable = input_w::<1>();

    let read0 = Regfile4x4_1R1W::apply(addr, write_enable, write_data, reset_all)[0];

    reset_all.set(1);
    write_enable.set_u8(0);
    simulate();
    reset_all.set(0);

    for i in 0..4 {
        addr[0].set_u8(i);
        simulate();
        assert_eq!(0, read0.get_u8());
    }

    // shuffle
    fn hash(v: &usize) -> f32 {
        let v = (*v as f32) * 4.567;
        v.sin()
    }
    let mut testcases: Vec<usize> = (0..256).collect();
    testcases.sort_by(|a, b| hash(a).total_cmp(&hash(b)));
    // println!("{:?}", testcases);

    let mut sim: [u8; 4] = [0, 0, 0, 0];
    for i in testcases {
        let a = (i % 4) as u8;
        let w = ((i >> 2) % 2) as u8;
        let v = ((i >> 3) % 16) as u8;

        addr[0].set_u8(a);
        write_data[0].set_u8(v);
        write_enable.set_u8(w);
        simulate();

        // println!(
        //     "a {}, w {}, v {}. {:?}. sim {}, read {}",
        //     a,
        //     w,
        //     v,
        //     sim,
        //     sim[a as usize],
        //     read0.get_u8()
        // );

        assert_eq!(sim[a as usize], read0.get_u8());

        if w == 1 {
            sim[a as usize] = v;
        }
    }
}

// pub struct Regfile4x4_2R1W;
// impl Regfile<2, 4, 2, 1> for Regfile4x4_2R1W {
//     fn apply(
//         addr: [Wires<2>; 2],
//         write_enable: Wires<1>,
//         write_data: [Wires<4>; 1],
//         reset_all: Wire,
//     ) -> [Wires<4>; 2] {
//         todo!()
//     }
// }
