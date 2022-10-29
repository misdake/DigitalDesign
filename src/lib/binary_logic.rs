use crate::{expand, nand, Wire, Wires};
use std::ops;

impl ops::Not for Wire {
    type Output = Wire;
    fn not(self) -> Self::Output {
        nand(self, self)
    }
}

impl ops::BitOr<Wire> for Wire {
    type Output = Wire;
    fn bitor(self, rhs: Wire) -> Self::Output {
        nand(!self, !rhs)
    }
}

impl ops::BitAnd<Wire> for Wire {
    type Output = Wire;
    fn bitand(self, rhs: Wire) -> Self::Output {
        !nand(self, rhs)
    }
}

impl ops::BitXor<Wire> for Wire {
    type Output = Wire;
    fn bitxor(self, rhs: Wire) -> Self::Output {
        let c = nand(self, rhs);
        nand(nand(self, c), nand(rhs, c))
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

/// select: 0 -> a, 1 -> b
pub fn mux2(a: Wire, b: Wire, select: Wire) -> Wire {
    (a & !select) | (b & select)
}

pub fn demux1(value: Wire, select: Wire) -> (Wire, Wire) {
    (value & !select, value & select) // (0, 1)
}

pub fn mux2_n<const W: usize>(a: &Wires<W>, b: &Wires<W>, select: Wire) -> Wires<W> {
    let select = &expand::<W>(select);
    &(a & &!select) | &(b & select)
}

pub fn mux4_n<const W: usize>(
    a: &Wires<W>,
    b: &Wires<W>,
    c: &Wires<W>,
    d: &Wires<W>,
    select: &Wires<2>,
) -> Wires<W> {
    let ab = mux2_n(a, b, select.wires[0]);
    let cd = mux2_n(c, d, select.wires[0]);
    mux2_n(&ab, &cd, select.wires[1])
}
