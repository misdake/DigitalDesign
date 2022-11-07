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
