use crate::{input_const, Wire, Wires};

#[derive(Copy, Clone)]
pub struct AddResult {
    pub sum: Wire,
    pub carry: Wire,
}

pub fn half_add(a: Wire, b: Wire) -> AddResult {
    AddResult {
        sum: a ^ b,
        carry: a & b,
    }
}

pub fn add(a: Wire, b: Wire, c: Wire) -> AddResult {
    let r1 = half_add(a, b);
    let r2 = half_add(r1.sum, c);
    AddResult {
        sum: r2.sum,
        carry: r1.carry | r2.carry,
    }
}

pub struct WiresAddResult<const W: usize> {
    pub sum: Wires<W>,
    pub carry: Wire,
}

pub fn add_naive<const W: usize>(a: &Wires<W>, b: &Wires<W>) -> WiresAddResult<W> {
    let mut carry = input_const(0);
    let mut out: [Wire; W] = [Wire(0); W];

    for i in 0..W {
        let r = add(a.wires[i], b.wires[i], carry);
        out[i] = r.sum;
        carry = r.carry;
    }

    WiresAddResult::<W> {
        sum: Wires { wires: out },
        carry,
    }
}
