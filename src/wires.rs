use crate::{input, input_const, nand, Wire};
use std::ops;

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
            .map(|(i, wire)| ((1 << i) * wire.get()) as u8)
            .reduce(|a, b| a + b)
            .unwrap()
    }
}

impl<'a, const W: usize> ops::Not for &'a Wires<W> {
    type Output = Wires<W>;
    fn not(self) -> Self::Output {
        let mut wires: [Wire; W] = [Wire(0); W];
        for i in 0..W {
            let w = self.wires[i];
            wires[i] = nand(w, w);
        }
        Wires::<W> { wires }
    }
}

impl<'a, 'b, const W: usize> ops::BitOr<&'b Wires<W>> for &'a Wires<W> {
    type Output = Wires<W>;
    fn bitor(self, rhs: &'b Wires<W>) -> Self::Output {
        let mut wires: [Wire; W] = [Wire(0); W];
        for i in 0..W {
            let a = self.wires[i];
            let b = rhs.wires[i];
            wires[i] = nand(!a, !b);
        }
        Wires::<W> { wires }
    }
}

impl<'a, 'b, const W: usize> ops::BitAnd<&'b Wires<W>> for &'a Wires<W> {
    type Output = Wires<W>;
    fn bitand(self, rhs: &'b Wires<W>) -> Self::Output {
        let mut wires: [Wire; W] = [Wire(0); W];
        for i in 0..W {
            let a = self.wires[i];
            let b = rhs.wires[i];
            wires[i] = !nand(a, b);
        }
        Wires::<W> { wires }
    }
}

impl<'a, 'b, const W: usize> ops::BitXor<&'b Wires<W>> for &'a Wires<W> {
    type Output = Wires<W>;
    fn bitxor(self, rhs: &'b Wires<W>) -> Self::Output {
        let mut wires: [Wire; W] = [Wire(0); W];
        for i in 0..W {
            let a = self.wires[i];
            let b = rhs.wires[i];
            let c = nand(a, b);
            wires[i] = nand(nand(a, c), nand(b, c))
        }
        Wires::<W> { wires }
    }
}
