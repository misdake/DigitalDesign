use crate::{mux2, reg, Wire};

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

#[test]
fn test_reg_w() {
    use crate::{add_naive, clear_all, input_w, reg_w, simulate};
    clear_all();

    let one = input_w::<4>();
    one.set_u8(1);
    let curr = reg_w::<4>();
    curr.set_in(add_naive(curr.out, one).sum);
    for i in 0..15 {
        simulate();
        assert_eq!(i + 1, curr.out.get_u8());
    }
}

#[test]
fn test_flipflop_w() {
    use crate::{clear_all, flipflop_w, input, input_w, simulate};
    clear_all();

    let d = input_w::<4>();
    let e = input();
    let q = flipflop_w(d, e);
    for i in 0..8 {
        d.set_u8(i);
        e.set(if i == 3 || i == 6 { 1 } else { 0 });
        simulate();
        if i >= 3 {
            assert_eq!(if i >= 6 { 6 } else { 3 }, q.get_u8());
        } else {
            assert_eq!(0, q.get_u8());
        }
    }
}
