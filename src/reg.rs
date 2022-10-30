use crate::{cycle, mux2, Wire, Wires};

pub fn reg(input: Wire) -> Wire {
    cycle(|_| input)
}

pub fn flipflop(data: Wire, write_enabled: Wire) -> Wire {
    cycle(|saved| mux2(saved, data, write_enabled))
}

pub fn flatten2<const A: usize, const B: usize>(a: &Wires<A>, b: &Wires<B>) -> Wires<{ A + B }> {
    let mut wires = [Wire(0); { A + B }];
    wires[0..A].copy_from_slice(a.wires.as_slice());
    wires[A..].copy_from_slice(b.wires.as_slice());
    Wires::<{ A + B }> { wires }
}

pub fn flatten3<const A: usize, const B: usize, const C: usize>(
    a: &Wires<A>,
    b: &Wires<B>,
    c: &Wires<C>,
) -> Wires<{ A + B + C }> {
    let mut wires = [Wire(0); { A + B + C }];
    wires[0..A].copy_from_slice(a.wires.as_slice());
    wires[A..A + B].copy_from_slice(b.wires.as_slice());
    wires[A + B..].copy_from_slice(c.wires.as_slice());
    Wires::<{ A + B + C }> { wires }
}

pub fn unflatten2<const A: usize, const B: usize>(r: &Wires<{ A + B }>) -> (Wires<A>, Wires<B>) {
    let wires = r.wires;
    let mut a = [Wire(0); A];
    let mut b = [Wire(0); B];
    for i in 0..A {
        a[i].0 = wires[i].0;
    }
    for i in 0..B {
        b[i].0 = wires[A + i].0;
    }
    (Wires::<A> { wires: a }, Wires::<B> { wires: b })
}

pub fn unflatten3<const A: usize, const B: usize, const C: usize>(
    r: &Wires<{ A + B + C }>,
) -> (Wires<A>, Wires<B>, Wires<C>) {
    let wires = r.wires;
    let mut a = [Wire(0); A];
    let mut b = [Wire(0); B];
    let mut c = [Wire(0); C];
    for i in 0..A {
        a[i].0 = wires[i].0;
    }
    for i in 0..B {
        b[i].0 = wires[A + i].0;
    }
    for i in 0..C {
        c[i].0 = wires[A + B + i].0;
    }
    (
        Wires::<A> { wires: a },
        Wires::<B> { wires: b },
        Wires::<C> { wires: c },
    )
}
