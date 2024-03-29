use crate::{input, input_const, mux2_w, reg, LatencyValue, Reg, Wire, WireValue};

pub enum Assert<const CHECK: bool> {}

pub trait IsTrue {}

impl IsTrue for Assert<true> {}

#[derive(Copy, Clone)]
pub struct Wires<const W: usize> {
    pub wires: [Wire; W],
}
impl<const W: usize> Wires<W> {
    pub fn width(&self) -> usize {
        W
    }
    pub fn uninitialized() -> Self {
        Self {
            wires: [Wire(0); W],
        }
    }
    pub fn set_latency(&self, latency: LatencyValue) {
        self.wires.iter().for_each(|w| w.set_latency(latency));
    }
    pub fn get_max_latency(&self) -> LatencyValue {
        self.wires
            .iter()
            .map(|w| w.get_latency())
            .max()
            .unwrap_or(0)
    }
}

pub fn input_w<const W: usize>() -> Wires<W> {
    let mut wires: [Wire; W] = [Wire(0); W];
    for i in 0..W {
        wires[i] = input();
    }
    Wires::<W> { wires }
}
pub fn input_w_const<const W: usize>(each_wire: WireValue) -> Wires<W> {
    let mut wires: [Wire; W] = [Wire(0); W];
    for i in 0..W {
        wires[i] = input_const(each_wire);
    }
    Wires::<W> { wires }
}
impl Wire {
    pub fn expand<const W: usize>(self) -> Wires<W> {
        Wires {
            wires: [Wire(self.0); W],
        }
    }
}

impl<const F: usize> Wires<F> {
    pub fn expand_unsigned<const T: usize>(&self) -> Wires<T>
    where
        Assert<{ F <= T }>: IsTrue,
    {
        let mut wires: [Wire; T] = [Wire(0); T];
        for i in 0..F {
            wires[i] = self.wires[i];
        }
        for i in F..T {
            wires[i] = input_const(0);
        }
        Wires::<T> { wires }
    }
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

pub trait WiresU8 {
    fn set_u8(&self, value: u8);
    fn get_u8(&self) -> u8;
}

impl<const W: usize> std::fmt::Debug for Wires<W>
where
    Assert<{ W <= 8 }>: IsTrue,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let v = self.get_u8();
        f.write_str(&format!("{v}({v:b})"))
    }
}

impl<const W: usize> WiresU8 for Wires<W>
where
    Assert<{ W <= 8 }>: IsTrue,
{
    fn set_u8(&self, value: u8) {
        self.set_u8(value);
    }

    fn get_u8(&self) -> u8 {
        self.get_u8()
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

#[derive(Copy, Clone)]
pub struct Regs<const W: usize> {
    regs: [Reg; W],
    pub out: Wires<W>,
}
impl<const W: usize> Regs<W> {
    pub fn width(&self) -> usize {
        W
    }

    pub fn set_in(&self, wires: Wires<W>) {
        for i in 0..W {
            self.regs[i].set_in(wires.wires[i]);
        }
    }
}
pub fn reg_w<const W: usize>() -> Regs<W> {
    let mut regs: [Reg; W] = [Reg(0); W];
    let mut out: [Wire; W] = [Wire(0); W];
    for i in 0..W {
        regs[i] = reg();
        out[i] = regs[i].out();
    }
    Regs::<W> {
        regs,
        out: Wires { wires: out },
    }
}

pub fn flipflop_w<const W: usize>(data: Wires<W>, write_enabled: Wire) -> Wires<W> {
    let r = reg_w();
    r.set_in(mux2_w(r.out, data, write_enabled));
    r.out
}
