use crate::{execute_all_gates, Wire};

pub fn test<F: Fn()>(a: Wire, f: F) {
    a.set(0);
    f();
    a.set(1);
    f();
}

pub fn test2_1(name: &str, a: Wire, b: Wire, out: Wire) {
    test(a, || {
        test(b, || {
            execute_all_gates();
            println!("{}({}, {}) = {}", name, a.get(), b.get(), out.get());
        });
    });
}

pub fn test3_2(name: &str, a: Wire, b: Wire, c: Wire, x: Wire, y: Wire) {
    test(a, || {
        test(b, || {
            test(c, || {
                execute_all_gates();
                println!(
                    "{}({}, {}, {}) = ({}, {})",
                    name,
                    a.get(),
                    b.get(),
                    c.get(),
                    x.get(),
                    y.get(),
                );
            });
        });
    });
}
