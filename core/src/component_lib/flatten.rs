use crate::{Wire, Wires};

pub fn flatten2<const A: usize, const B: usize>(a: Wires<A>, b: Wires<B>) -> Wires<{ A + B }> {
    let mut wires = [Wire(0); { A + B }];
    wires[0..A].copy_from_slice(a.wires.as_slice());
    wires[A..].copy_from_slice(b.wires.as_slice());
    Wires::<{ A + B }> { wires }
}

pub fn flatten3<const A: usize, const B: usize, const C: usize>(
    a: Wires<A>,
    b: Wires<B>,
    c: Wires<C>,
) -> Wires<{ A + B + C }> {
    let mut wires = [Wire(0); { A + B + C }];
    wires[0..A].copy_from_slice(a.wires.as_slice());
    wires[A..A + B].copy_from_slice(b.wires.as_slice());
    wires[A + B..].copy_from_slice(c.wires.as_slice());
    Wires::<{ A + B + C }> { wires }
}

pub fn unflatten2<const A: usize, const B: usize>(r: Wires<{ A + B }>) -> (Wires<A>, Wires<B>) {
    let wires = r.wires;
    let mut a = [Wire(0); A];
    let mut b = [Wire(0); B];
    for i in 0..A {
        a[i] = wires[i];
    }
    for i in 0..B {
        b[i] = wires[A + i];
    }
    (Wires::<A> { wires: a }, Wires::<B> { wires: b })
}

pub fn unflatten3<const A: usize, const B: usize, const C: usize>(
    r: Wires<{ A + B + C }>,
) -> (Wires<A>, Wires<B>, Wires<C>) {
    let wires = r.wires;
    let mut a = [Wire(0); A];
    let mut b = [Wire(0); B];
    let mut c = [Wire(0); C];
    for i in 0..A {
        a[i] = wires[i];
    }
    for i in 0..B {
        b[i] = wires[A + i];
    }
    for i in 0..C {
        c[i] = wires[A + B + i];
    }
    (
        Wires::<A> { wires: a },
        Wires::<B> { wires: b },
        Wires::<C> { wires: c },
    )
}

#[test]
fn test_flatten_unflatten() {
    use crate::{add_naive, build_circuit, flatten3, input_w, unflatten3};

    let (mut circuit, (a, b, c, d, f, x, y, z, r)) = build_circuit(|| {
        let a = input_w::<2>();
        let b = input_w::<3>();
        let c = input_w::<3>();
        let d = input_w::<8>();
        let f = flatten3(a, b, c);
        let (x, y, z) = unflatten3::<2, 3, 3>(f);
        let r = add_naive(d, f); // 105 + 46 = 151
        (a, b, c, d, f, x, y, z, r)
    });
    circuit.set_wires_u8(a, 1);
    circuit.set_wires_u8(b, 2);
    circuit.set_wires_u8(c, 3);
    circuit.set_wires_u8(d, 46);
    assert_eq!(105, circuit.get_wires_u8(f)); // 1 + 2<<2 + 3<<5
    circuit.simulate();
    assert_eq!(151, circuit.get_wires_u8(r.sum));
    assert_eq!(1, circuit.get_wires_u8(x));
    assert_eq!(2, circuit.get_wires_u8(y));
    assert_eq!(3, circuit.get_wires_u8(z));

    circuit.set_wires_u8(d, 0b11010010);
    let (x, y, z) = unflatten3::<2, 2, 4>(d);
    assert_eq!(0b10, circuit.get_wires_u8(x));
    assert_eq!(0b00, circuit.get_wires_u8(y));
    assert_eq!(0b1101, circuit.get_wires_u8(z));
}
