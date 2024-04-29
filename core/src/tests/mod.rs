pub fn shuffled_list(count: usize, seed: f32) -> Vec<u32> {
    let hash = |v: &u32| -> f32 {
        let v = (*v as f32) * seed;
        v.sin()
    };
    let mut r: Vec<u32> = (0..count).map(|i| i as u32).collect();
    r.sort_by(|a, b| hash(a).total_cmp(&hash(b)));
    // println!("{:?}", r);
    r
}

#[cfg(test)]
use crate::{Circuit, Wire, WireValue};

#[cfg(test)]
pub fn test<F: Fn(&mut Circuit)>(circuit: &mut Circuit, a: Wire, f: F) {
    circuit.set_wire(a, 0);
    f(circuit);
    circuit.set_wire(a, 1);
    f(circuit);
}

#[cfg(test)]
pub fn test2_1(
    c: &mut Circuit,
    name: &str,
    a: Wire,
    b: Wire,
    out: Wire,
    f: fn(a: WireValue, b: WireValue) -> WireValue,
) {
    test(c, a, |c| {
        test(c, b, |c| {
            c.simulate();
            println!(
                "{}({}, {}) = {}",
                name,
                c.get_wire(a),
                c.get_wire(b),
                c.get_wire(out)
            );
            assert_eq!(c.get_wire(out), f(c.get_wire(a), c.get_wire(b)));
        });
    });
}
