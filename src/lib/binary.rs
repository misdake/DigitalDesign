use crate::{nand, Wire, Wires};
use std::ops;

impl Wires<2> {
    pub fn all_0(self) -> Wire {
        !(self.wires[0] | self.wires[1])
    }
    pub fn all_1(self) -> Wire {
        self.wires[0] & self.wires[1]
    }
    pub fn eq(self, rhs: Wires<2>) -> Wire {
        (self ^ rhs).all_0()
    }
}
impl Wires<4> {
    pub fn all_0(self) -> Wire {
        !((self.wires[0] | self.wires[1]) | (self.wires[2] | self.wires[3]))
    }
    pub fn all_1(self) -> Wire {
        (self.wires[0] & self.wires[1]) & (self.wires[2] & self.wires[3])
    }
    pub fn eq(self, rhs: Wires<4>) -> Wire {
        (self ^ rhs).all_0()
    }
}
impl Wires<8> {
    pub fn all_0(self) -> Wire {
        !(((self.wires[0] | self.wires[1]) | (self.wires[2] | self.wires[3]))
            | ((self.wires[4] | self.wires[5]) | (self.wires[6] | self.wires[7])))
    }
    pub fn all_1(self) -> Wire {
        ((self.wires[0] & self.wires[1]) & (self.wires[2] & self.wires[3]))
            & ((self.wires[4] & self.wires[5]) & (self.wires[6] & self.wires[7]))
    }
    pub fn eq(self, rhs: Wires<8>) -> Wire {
        (self ^ rhs).all_0()
    }
}

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

impl<const W: usize> ops::Not for Wires<W> {
    type Output = Wires<W>;
    fn not(self) -> Self::Output {
        let mut wires: [Wire; W] = [Wire(0); W];
        for i in 0..W {
            let w = self.wires[i];
            wires[i] = !w;
        }
        Wires::<W> { wires }
    }
}

impl<const W: usize> ops::BitOr<Wires<W>> for Wires<W> {
    type Output = Wires<W>;
    fn bitor(self, rhs: Wires<W>) -> Self::Output {
        let mut wires: [Wire; W] = [Wire(0); W];
        for i in 0..W {
            let a = self.wires[i];
            let b = rhs.wires[i];
            wires[i] = a | b;
        }
        Wires::<W> { wires }
    }
}

impl<const W: usize> ops::BitAnd<Wires<W>> for Wires<W> {
    type Output = Wires<W>;
    fn bitand(self, rhs: Wires<W>) -> Self::Output {
        let mut wires: [Wire; W] = [Wire(0); W];
        for i in 0..W {
            let a = self.wires[i];
            let b = rhs.wires[i];
            wires[i] = a & b;
        }
        Wires::<W> { wires }
    }
}

impl<const W: usize> ops::BitXor<Wires<W>> for Wires<W> {
    type Output = Wires<W>;
    fn bitxor(self, rhs: Wires<W>) -> Self::Output {
        let mut wires: [Wire; W] = [Wire(0); W];
        for i in 0..W {
            let a = self.wires[i];
            let b = rhs.wires[i];
            wires[i] = a ^ b;
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

pub fn mux2_w<const W: usize>(a: Wires<W>, b: Wires<W>, select: Wire) -> Wires<W> {
    let select = select.expand::<W>();
    (a & !select) | (b & select)
}

pub fn mux4_w<const W: usize>(
    a: Wires<W>,
    b: Wires<W>,
    c: Wires<W>,
    d: Wires<W>,
    select: Wires<2>,
) -> Wires<W> {
    let ab = mux2_w(a, b, select.wires[0]);
    let cd = mux2_w(c, d, select.wires[0]);
    mux2_w(ab, cd, select.wires[1])
}

pub fn mux8_w<const W: usize>(v: &[Wires<W>], select: Wires<3>) -> Wires<W> {
    let select4 = Wires {
        wires: [select.wires[0], select.wires[1]],
    };
    let v0 = mux4_w(v[0], v[1], v[2], v[3], select4);
    let v1 = mux4_w(v[4], v[5], v[6], v[7], select4);
    mux2_w(v0, v1, select.wires[2])
}
pub fn mux16_w<const W: usize>(v: &[Wires<W>], select: Wires<4>) -> Wires<W> {
    let select8 = Wires {
        wires: [select.wires[0], select.wires[1], select.wires[2]],
    };
    let v0 = mux8_w(&v[0..8], select8);
    let v1 = mux8_w(&v[8..16], select8);
    mux2_w(v0, v1, select.wires[3])
}
pub fn mux16_w_v2<const W: usize>(v: &[Wires<W>], select4: Wires<4>) -> Wires<W> {
    let t = select4;
    let f = !t;
    let mut lines: [Wires<W>; 16] = [Wires {
        wires: [Wire(0); W],
    }; 16];
    for i in 0..16 {
        let s = Wires {
            wires: [
                select(i & (1 << 0) == 0, f.wires[0], t.wires[0]),
                select(i & (1 << 1) == 0, f.wires[1], t.wires[1]),
                select(i & (1 << 2) == 0, f.wires[2], t.wires[2]),
                select(i & (1 << 3) == 0, f.wires[3], t.wires[3]),
            ],
        };
        lines[i] = s.all_1().expand() & v[i];
    }
    reduce16(lines.as_slice(), &|a, b| a | b)
}

fn select<T>(b: bool, t: T, f: T) -> T {
    if b {
        t
    } else {
        f
    }
}

pub fn reduce2<const W: usize>(
    a: Wires<W>,
    b: Wires<W>,
    f: &impl Fn(Wires<W>, Wires<W>) -> Wires<W>,
) -> Wires<W> {
    f(a, b)
}
pub fn reduce4<const W: usize>(
    v: &[Wires<W>],
    f: &impl Fn(Wires<W>, Wires<W>) -> Wires<W>,
) -> Wires<W> {
    let v0 = reduce2(v[0], v[1], f);
    let v1 = reduce2(v[2], v[3], f);
    f(v0, v1)
}
pub fn reduce8<const W: usize>(
    v: &[Wires<W>],
    f: &impl Fn(Wires<W>, Wires<W>) -> Wires<W>,
) -> Wires<W> {
    let v0 = reduce4(&v[0..4], f);
    let v1 = reduce4(&v[4..8], f);
    f(v0, v1)
}
pub fn reduce16<const W: usize>(
    v: &[Wires<W>],
    f: &impl Fn(Wires<W>, Wires<W>) -> Wires<W>,
) -> Wires<W> {
    let v0 = reduce8(&v[0..8], f);
    let v1 = reduce8(&v[8..16], f);
    f(v0, v1)
}
