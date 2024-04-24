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

pub fn add1(a: Wire, b: Wire, c: Wire) -> AddResult {
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

pub fn add_naive<const W: usize>(a: Wires<W>, b: Wires<W>) -> WiresAddResult<W> {
    let mut carry = input_const(0);
    let mut out: [Wire; W] = [Wire(0); W];

    #[allow(clippy::needless_range_loop)]
    for i in 0..W {
        let r = add1(a.wires[i], b.wires[i], carry);
        out[i] = r.sum;
        carry = r.carry;
    }

    WiresAddResult::<W> {
        sum: Wires { wires: out },
        carry,
    }
}

#[test]
fn test_add_naive() {
    use crate::{add_naive, build_circuit, build_circuit, input_w};
    let (mut circuit, (a, b, c, d)) = build_circuit(|| {
        let a = input_w::<8>();
        let b = input_w::<8>();
        let c = a & b;
        let d = add_naive(a, b);
        (a, b, c, d)
    });
    circuit.set_wires_u8(a, 123);
    circuit.set_wires_u8(b, 45);
    assert_eq!(123, circuit.get_wires_u8(a));
    assert_eq!(45, circuit.get_wires_u8(b));
    circuit.simulate();
    println!("adder {:?}", circuit.get_statistics());
    assert_eq!(0b101001, circuit.get_wires_u8(c));
    assert_eq!(168, circuit.get_wires_u8(d.sum));
    assert_eq!(0, circuit.get_wire(d.carry));
}
