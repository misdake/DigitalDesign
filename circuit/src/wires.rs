use crate::{input, input_const, mux2_w, reg, CircuitWires, LatencyValue, Reg, Wire, WireValue};

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

    pub fn set_latency_external(self, latency: LatencyValue) {
        self.wires
            .into_iter()
            .for_each(|wire| wire.set_latency_external(latency));
    }
    pub fn get_max_latency_external(self) -> LatencyValue {
        self.wires
            .into_iter()
            .map(|w| w.get_latency_external())
            .max()
            .unwrap_or(0)
    }
}
impl CircuitWires {
    pub fn get_max_latency<const W: usize>(&mut self, wires: Wires<W>) -> LatencyValue {
        wires
            .wires
            .into_iter()
            .map(|w| self.get_wire_latency(w))
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
    fn set_u8(&self, circuit: &mut CircuitWires, value: u8);
    fn get_u8(&self, circuit: &CircuitWires) -> u8;
}

pub trait WiresU16 {
    fn set_u16(&self, circuit: &mut CircuitWires, value: u16);
    fn get_u16(&self, circuit: &CircuitWires) -> u16;
}

//TODO how?
// impl<const W: usize> std::fmt::Debug for Wires<W>
// where
//     Assert<{ W <= 8 }>: IsTrue,
// {
//     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//         let v = self.get_u8();
//         f.write_str(&format!("{v}({v:b})"))
//     }
// }

impl<const W: usize> WiresU8 for Wires<W>
where
    Assert<{ W <= 8 }>: IsTrue,
{
    fn set_u8(&self, circuit: &mut CircuitWires, value: u8) {
        for i in 0..W {
            circuit.set_wire(self.wires[i], ((value & (1 << i)) > 0).into());
        }
    }

    fn get_u8(&self, circuit: &CircuitWires) -> u8 {
        self.wires
            .iter()
            .enumerate()
            .map(|(i, wire)| ((1 << i) * circuit.get_wire(*wire)) as WireValue)
            .reduce(|a, b| a + b)
            .unwrap()
    }
}

impl<const W: usize> WiresU16 for Wires<W>
where
    Assert<{ W <= 16 }>: IsTrue,
{
    fn set_u16(&self, circuit: &mut CircuitWires, value: u16) {
        for i in 0..W {
            circuit.set_wire(self.wires[i], ((value & (1u16 << i)) != 0).into());
        }
    }

    fn get_u16(&self, circuit: &CircuitWires) -> u16 {
        self.wires.iter().enumerate().fold(0, |value, (i, wire)| {
            value | ((circuit.get_wire(*wire) as u16) << i)
        })
    }
}

impl<const W: usize> Wires<W>
where
    Assert<{ W <= 8 }>: IsTrue,
{
    pub fn parse_u8(value: u8) -> Wires<W> {
        let mut wires = [Wire(0); W];
        for i in 0..W {
            wires[i] = input_const(((value & (1 << i)) > 0).into());
        }
        Wires::<W> { wires }
    }
}

impl<const W: usize> Wires<W>
where
    Assert<{ W <= 16 }>: IsTrue,
{
    pub fn parse_u16(value: u16) -> Wires<W> {
        let mut wires = [Wire(0); W];
        for i in 0..W {
            wires[i] = input_const(((value & (1u16 << i)) != 0).into());
        }
        Wires::<W> { wires }
    }
}

impl CircuitWires {
    // constraints don't work outside crate
    pub fn set_wires_u8<const W: usize>(&mut self, wires: Wires<W>, value: u8)
    where
        Assert<{ W <= 8 }>: IsTrue,
    {
        wires.set_u8(self, value)
    }
    pub fn get_wires_u8<const W: usize>(&self, wires: Wires<W>) -> u8
    where
        Assert<{ W <= 8 }>: IsTrue,
    {
        wires.get_u8(self)
    }
    pub fn set_wires_u16<const W: usize>(&mut self, wires: Wires<W>, value: u16)
    where
        Assert<{ W <= 16 }>: IsTrue,
    {
        wires.set_u16(self, value)
    }
    pub fn get_wires_u16<const W: usize>(&self, wires: Wires<W>) -> u16
    where
        Assert<{ W <= 16 }>: IsTrue,
    {
        wires.get_u16(self)
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

#[test]
fn test_wires_u16() {
    use crate::build_circuit;

    let (mut circuit, (input, constant)) =
        build_circuit(|| (input_w::<16>(), Wires::<16>::parse_u16(0xa55a)));

    input.set_u16(&mut circuit, 0x5aa5);
    circuit.execute_gates();

    assert_eq!(input.get_u16(&circuit), 0x5aa5);
    assert_eq!(constant.get_u16(&circuit), 0xa55a);
}
