use crate::{mux2, reg, Wire};

pub fn delay(input: Wire) -> Wire {
    let r = reg();
    r.set_in(input);
    r.out()
}

pub fn flipflop(data: Wire, write_enabled: Wire) -> Wire {
    let r = reg();
    r.set_in(mux2(r.out(), data, write_enabled));
    r.out()
}

#[cfg(test)]
use crate::build_circuit;

#[test]
fn test_reg() {
    use crate::{delay, input, reg};

    let (mut circuit, (a, b, c, d)) = build_circuit(|| {
        let a = input();
        let r = reg();
        let b = r.out();
        r.set_in(a | b);
        let c = delay(a);
        let d = delay(c);
        (a, b, c, d)
    });
    for i in 0..10 {
        circuit.set_wire(a, if i == 5 { 1 } else { 0 });
        circuit.simulate();
        assert_eq!(if i >= 5 { 1 } else { 0 }, circuit.get_wire(b));
        assert_eq!(if i == 5 { 1 } else { 0 }, circuit.get_wire(c));
        assert_eq!(if i == 6 { 1 } else { 0 }, circuit.get_wire(d));
    }
}

#[test]
fn test_flipflop() {
    use crate::{flipflop, input};
    let (mut circuit, (d, e, q)) = build_circuit(|| {
        let d = input();
        let e = input();
        let q = flipflop(d, e);
        (d, e, q)
    });
    for i in 0..20 {
        circuit.set_wire(d, if i < 5 || i > 12 { 0 } else { 1 });
        circuit.set_wire(e, if i == 9 || i == 15 { 1 } else { 0 });
        circuit.simulate();
        assert_eq!(if i >= 9 && i <= 14 { 1 } else { 0 }, circuit.get_wire(q));
    }
}

#[test]
fn test_reg_w() {
    use crate::{add_naive, input_w, reg_w};

    let (mut circuit, (one, curr)) = build_circuit(|| {
        let one = input_w::<4>();
        let curr = reg_w::<4>();
        curr.set_in(add_naive(curr.out, one).sum);
        (one, curr)
    });

    circuit.set_wires_u8(one, 1);

    for i in 0..15 {
        circuit.simulate();
        assert_eq!(i + 1, circuit.get_wires_u8(curr.out));
    }
}

#[test]
fn test_flipflop_w() {
    use crate::{flipflop_w, input, input_w};

    let (mut circuit, (d, e, q)) = build_circuit(|| {
        let d = input_w::<4>();
        let e = input();
        let q = flipflop_w(d, e);
        (d, e, q)
    });
    for i in 0..8 {
        circuit.set_wires_u8(d, i);
        circuit.set_wire(e, if i == 3 || i == 6 { 1 } else { 0 });
        circuit.simulate();
        if i >= 3 {
            assert_eq!(if i >= 6 { 6 } else { 3 }, circuit.get_wires_u8(q));
        } else {
            assert_eq!(0, circuit.get_wires_u8(q));
        }
    }
}
