use crate::{Wire, Wires};

pub trait RegFile<const ADDR: usize, const WIDTH: usize, const READ: usize, const WRITE: usize> {
    fn apply(
        addr: [Wires<ADDR>; READ],
        write_enable: Wires<WRITE>,
        write_data: [Wires<WIDTH>; WRITE],
        reset_all: Wire,
    ) -> [Wires<WIDTH>; READ];
}

pub struct RegFile4x4_1R1W;
impl RegFile<2, 4, 1, 1> for RegFile4x4_1R1W {
    fn apply(
        addr: [Wires<2>; 1],
        write_enable: Wires<1>,
        write_data: [Wires<4>; 1],
        reset_all: Wire,
    ) -> [Wires<4>; 1] {
        todo!()
    }
}

pub struct RegFile4x4_2R1W;
impl RegFile<2, 4, 2, 1> for RegFile4x4_2R1W {
    fn apply(
        addr: [Wires<2>; 2],
        write_enable: Wires<1>,
        write_data: [Wires<4>; 1],
        reset_all: Wire,
    ) -> [Wires<4>; 2] {
        todo!()
    }
}
