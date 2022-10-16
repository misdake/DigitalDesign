use crate::{execute_all_gates, Wire};

pub fn test1_1<F: Fn()>(a: Wire, f: F) {
    a.set(0);
    f();
    a.set(1);
    f();
}

pub fn test2_1(name: &str, a: Wire, b: Wire, out: Wire) {
    test1_1(a, || {
        test1_1(b, || {
            execute_all_gates();
            println!("{}({}, {}) = {}", name, a.get(), b.get(), out.get());
        });
    });
}

pub fn test3_2(name: &str, a: Wire, b: Wire, c: Wire, x: Wire, y: Wire) {
    test1_1(a, || {
        test1_1(b, || {
            test1_1(c, || {
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
