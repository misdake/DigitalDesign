use crate::{input, input_const, Wire, WireValue};

pub enum Assert<const CHECK: bool> {}

pub trait IsTrue {}

impl IsTrue for Assert<true> {}

#[derive(Debug, Clone)]
pub struct Wires<const W: usize> {
    pub wires: [Wire; W],
}

pub fn input_w<const W: usize>() -> Wires<W> {
    let mut wires: [Wire; W] = [Wire(0); W];
    for i in 0..W {
        wires[i] = input();
    }
    Wires::<W> { wires }
}
pub fn input_w_const<const W: usize>(value: WireValue) -> Wires<W> {
    let mut wires: [Wire; W] = [Wire(0); W];
    for i in 0..W {
        wires[i] = input_const(value);
    }
    Wires::<W> { wires }
}

impl<const F: usize> Wires<F> {
    pub fn expand_signed<const T: usize>(&self) -> Wires<T>
    where
        Assert<{ F <= T }>: IsTrue,
    {
        let mut wires: [Wire; T] = [Wire(0); T];
        for i in 0..F {
            wires[i] = self.wires[i];
        }
        for i in F..T {
            wires[i] = self.wires[F - 1];
        }
        Wires::<T> { wires }
    }
}

impl<const W: usize> Wires<W>
where
    Assert<{ W <= 8 }>: IsTrue,
{
    pub fn parse_u8(value: u8) -> Wires<W> {
        let wires: [Wire; W] = (0..W)
            .map(|i| input_const(((value & (1 << i)) > 0).into()))
            .collect::<Vec<Wire>>()
            .try_into()
            .unwrap();
        Wires::<W> { wires }
    }

    pub fn set_u8(&self, value: u8) {
        for i in 0..W {
            self.wires[i].set(((value & (1 << i)) > 0).into());
        }
    }

    pub fn get_u8(&self) -> u8 {
        self.wires
            .iter()
            .enumerate()
            .map(|(i, wire)| ((1 << i) * wire.get()) as WireValue)
            .reduce(|a, b| a + b)
            .unwrap()
    }
}
