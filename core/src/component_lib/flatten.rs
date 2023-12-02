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
    use crate::{add_naive, clear_all, flatten3, input_w, simulate, unflatten3};
    clear_all();

    let a = input_w::<2>();
    let b = input_w::<3>();
    let c = input_w::<3>();
    let d = input_w::<8>();
    a.set_u8(1);
    b.set_u8(2);
    c.set_u8(3);
    d.set_u8(46);
    let f = flatten3(a, b, c);
    assert_eq!(105, f.get_u8()); // 1 + 2<<2 + 3<<5
    let r = add_naive(d, f); // 105 + 46 = 151
    simulate();
    assert_eq!(151, r.sum.get_u8());
    let (x, y, z) = unflatten3::<2, 3, 3>(f);
    assert_eq!(1, x.get_u8());
    assert_eq!(2, y.get_u8());
    assert_eq!(3, z.get_u8());

    d.set_u8(0b11010010);
    let (x, y, z) = unflatten3::<2, 2, 4>(d);
    assert_eq!(0b10, x.get_u8());
    assert_eq!(0b00, y.get_u8());
    assert_eq!(0b1101, z.get_u8());
}
