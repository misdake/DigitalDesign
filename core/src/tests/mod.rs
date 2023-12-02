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
use crate::{simulate, Wire, WireValue};

#[cfg(test)]
pub fn test<F: Fn()>(a: Wire, f: F) {
    a.set(0);
    f();
    a.set(1);
    f();
}

#[cfg(test)]
pub fn test2_1(
    name: &str,
    a: Wire,
    b: Wire,
    out: Wire,
    f: fn(a: WireValue, b: WireValue) -> WireValue,
) {
    test(a, || {
        test(b, || {
            simulate();
            println!("{}({}, {}) = {}", name, a.get(), b.get(), out.get());
            assert_eq!(out.get(), f(a.get(), b.get()));
        });
    });
}
