use crate::{decode2, input_w_const, mux2_w, reduce4, reg_w, Regs, Wire, Wires};

pub trait Regfile<const ADDR: usize, const WIDTH: usize, const READ: usize, const WRITE: usize> {
    fn create_regs() -> [Regs<WIDTH>; 1 << ADDR] {
        [0; 1 << ADDR].map(|_| reg_w())
    }
    fn apply(
        regs: [Regs<WIDTH>; 1 << ADDR],
        addr: [Wires<ADDR>; READ],
        write_enable: Wires<WRITE>,
        write_data: [Wires<WIDTH>; WRITE],
        reset_all: Wire,
    ) -> [Wires<WIDTH>; READ];
}

pub struct Regfile4x4_1R1W;
impl Regfile<2, 4, 1, 1> for Regfile4x4_1R1W {
    fn apply(
        regs: [Regs<4>; 1 << 2],
        addr: [Wires<2>; 1],
        write_enable: Wires<1>,
        write_data: [Wires<4>; 1],
        reset_all: Wire,
    ) -> [Wires<4>; 1] {
        let port0_enable_each = decode2(addr[0]);

        let mut port0_read_each: [Wires<4>; 4] = [Wires {
            wires: [Wire(0); 4],
        }; 4];

        for i in 0..4 {
            let port0_enable = port0_enable_each[i];
            port0_read_each[i] = port0_enable.expand() & regs[i].out;

            let port0_write_data0 = mux2_w(regs[i].out, input_w_const(0), reset_all);
            let port0_write_data1 = write_data[0];
            let port0_write_enable = port0_enable & write_enable.wires[0];
            let port0_write_data = mux2_w(port0_write_data0, port0_write_data1, port0_write_enable);
            regs[i].set_in(port0_write_data)
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

    let regs = Regfile4x4_1R1W::create_regs();
    let read0 = Regfile4x4_1R1W::apply(regs, addr, write_enable, write_data, reset_all)[0];

    reset_all.set(1);
    write_enable.set_u8(0);
    simulate();
    reset_all.set(0);

    for i in 0..4 {
        addr[0].set_u8(i);
        simulate();
        assert_eq!(0, read0.get_u8());
    }

    let testcases = shuffled_list(1 << 7, 4.567);

    let mut sim: [u8; 4] = [0, 0, 0, 0];
    for i in testcases {
        // 7bits per testcase:
        let a = (i % 4) as u8; // 2bits
        let w = ((i >> 2) % 2) as u8; // 1bit
        let v = ((i >> 3) % 16) as u8; // 4bits

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

    reset_all.set(1);
    write_enable.set_u8(0);
    simulate();
    reset_all.set(0);

    for i in 0..4 {
        addr[0].set_u8(i);
        simulate();
        assert_eq!(0, read0.get_u8());
    }
}

pub struct Regfile4x4_2R1W;
impl Regfile<2, 4, 2, 1> for Regfile4x4_2R1W {
    fn apply(
        regs: [Regs<4>; 1 << 2],
        addr: [Wires<2>; 2],
        write_enable: Wires<1>,
        write_data: [Wires<4>; 1],
        reset_all: Wire,
    ) -> [Wires<4>; 2] {
        let port0_enable_each = decode2(addr[0]);
        let port1_enable_each = decode2(addr[1]);

        let mut port0_read_each: [Wires<4>; 4] = [Wires {
            wires: [Wire(0); 4],
        }; 4];
        let mut port1_read_each: [Wires<4>; 4] = [Wires {
            wires: [Wire(0); 4],
        }; 4];

        for i in 0..4 {
            let port0_enable = port0_enable_each[i];
            let port1_enable = port1_enable_each[i];
            port0_read_each[i] = port0_enable.expand() & regs[i].out;
            port1_read_each[i] = port1_enable.expand() & regs[i].out;

            let port0_write_data0 = mux2_w(regs[i].out, input_w_const(0), reset_all);
            let port0_write_data1 = write_data[0];
            let port0_write_enable = port0_enable & write_enable.wires[0];
            let port0_write_data = mux2_w(port0_write_data0, port0_write_data1, port0_write_enable);
            regs[i].set_in(port0_write_data)
        }

        let port0_read = reduce4(port0_read_each.as_slice(), &|a, b| a | b);
        let port1_read = reduce4(port1_read_each.as_slice(), &|a, b| a | b);
        [port0_read, port1_read]
    }
}
#[test]
fn test_regfile4x4_2r1w() {
    use crate::*;
    clear_all();

    let reset_all = input();
    let addr = [input_w::<2>(), input_w::<2>()];
    let write_data = [input_w::<4>()];
    let write_enable = input_w::<1>();

    let regs = Regfile4x4_2R1W::create_regs();
    let [read0, read1] = Regfile4x4_2R1W::apply(regs, addr, write_enable, write_data, reset_all);

    reset_all.set(1);
    write_enable.set_u8(0);
    simulate();
    reset_all.set(0);

    for i in 0..4 {
        addr[0].set_u8(i);
        simulate();
        assert_eq!(0, read0.get_u8());
    }

    let testcases = shuffled_list(1 << 9, 4.567);

    let mut sim: [u8; 4] = [0, 0, 0, 0];
    for i in testcases {
        // 9bits per testcase:
        let a0 = (i % 4) as u8; // 2bits
        let a1 = ((i >> 2) % 4) as u8; // 2bits
        let w = ((i >> 4) % 2) as u8; // 1bit
        let v = ((i >> 5) % 16) as u8; // 4bits

        addr[0].set_u8(a0);
        addr[1].set_u8(a1);
        write_data[0].set_u8(v);
        write_enable.set_u8(w);
        simulate();

        assert_eq!(sim[a0 as usize], read0.get_u8());
        assert_eq!(sim[a1 as usize], read1.get_u8());

        if w == 1 {
            sim[a0 as usize] = v;
        }
    }

    reset_all.set(1);
    write_enable.set_u8(0);
    simulate();
    reset_all.set(0);

    for i in 0..4 {
        addr[1].set_u8(i);
        simulate();
        assert_eq!(0, read1.get_u8());
    }
}
