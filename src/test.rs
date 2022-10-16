use crate::{execute_all_gates, Wire};

pub fn test2(name: &str, a: Wire, b: Wire, out: Wire) {
    a.set(0);
    b.set(0);
    execute_all_gates();
    println!("{}({}, {}) = {}", name, a.get(), b.get(), out.get());
    a.set(1);
    b.set(0);
    execute_all_gates();
    println!("{}({}, {}) = {}", name, a.get(), b.get(), out.get());
    a.set(0);
    b.set(1);
    execute_all_gates();
    println!("{}({}, {}) = {}", name, a.get(), b.get(), out.get());
    a.set(1);
    b.set(1);
    execute_all_gates();
    println!("{}({}, {}) = {}", name, a.get(), b.get(), out.get());
}
