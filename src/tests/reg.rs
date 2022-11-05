#[test]
fn test_reg() {
    use crate::{clear_all, delay, input, reg, simulate};
    clear_all();

    let a = input();
    let r = reg();
    let b = r.out();
    r.set_in(a | b);
    let c = delay(a);
    let d = delay(c);
    for i in 0..10 {
        a.set(if i == 5 { 1 } else { 0 });
        simulate();
        assert_eq!(if i >= 5 { 1 } else { 0 }, b.get());
        assert_eq!(if i == 5 { 1 } else { 0 }, c.get());
        assert_eq!(if i == 6 { 1 } else { 0 }, d.get());
    }
}

#[test]
fn test_flipflop() {
    use crate::{clear_all, flipflop, input, simulate};
    clear_all();

    // flipflop
    let d = input();
    let e = input();
    let q = flipflop(d, e);
    for i in 0..20 {
        d.set(if i < 5 || i > 12 { 0 } else { 1 });
        e.set(if i == 9 || i == 15 { 1 } else { 0 });
        simulate();
        assert_eq!(if i >= 9 && i <= 14 { 1 } else { 0 }, q.get());
    }
}
