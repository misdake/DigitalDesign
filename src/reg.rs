use crate::{mux2, reg, Wire, Wires};

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
