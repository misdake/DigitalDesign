use crate::{simulate, Wires};

#[test]
fn test_basic_binary() {
    use crate::tests::test_utils::test2_1;
    use crate::{clear_all, input, nand};
    clear_all();

    let a = input();
    let b = input();

    let c = nand(a, b);
    let d = a & b;
    let e = a | b;
    let f = a ^ b;

    test2_1("nand", a, b, c, |a, b| !(a & b) & 1);
    test2_1("and", a, b, d, |a, b| a & b);
    test2_1("or", a, b, e, |a, b| a | b);
    test2_1("xor", a, b, f, |a, b| a ^ b);
}

#[test]
fn test_wire_eq() {
    use crate::{clear_all, input_w};
    clear_all();

    let v = input_w::<4>();
    for i in 0..16 {
        v.set_u8(i);
        let out1 = v.eq_const(5);
        let out2 = v.eq(Wires::<4>::parse_u8(5));
        simulate();
        assert_eq!(out1.get() == 1, i == 5);
        assert_eq!(out2.get() == 1, i == 5);
    }
}

#[test]
fn test_expand_signed() {
    use crate::{clear_all, input_w};
    clear_all();

    let a = &input_w::<4>();
    let b = &a.expand_signed::<8>();
    a.set_u8(5);
    assert_eq!(0b00000101, b.get_u8());
    a.set_u8(9);
    assert_eq!(0b11111001, b.get_u8());
}
