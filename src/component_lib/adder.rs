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
    use crate::{add_naive, clear_all, get_statistics, input_w, simulate};
    clear_all();

    let a = input_w::<8>();
    let b = input_w::<8>();
    a.set_u8(123);
    b.set_u8(45);
    assert_eq!(123, a.get_u8());
    assert_eq!(45, b.get_u8());
    let c = a & b;
    let d = add_naive(a, b);
    simulate();
    println!("adder {:?}", get_statistics());
    assert_eq!(0b101001, c.get_u8());
    assert_eq!(168, d.sum.get_u8());
    assert_eq!(0, d.carry.get());
}
