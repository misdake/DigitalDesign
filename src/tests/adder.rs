#[test]
fn test_add_naive() {
    use crate::{add_naive, clear_all, input_w, simulate};
    clear_all();

    let a = &input_w::<8>();
    let b = &input_w::<8>();
    a.set_u8(123);
    b.set_u8(45);
    assert_eq!(123, a.get_u8());
    assert_eq!(45, b.get_u8());
    let c = a & b;
    let d = add_naive(a, b);
    simulate();
    assert_eq!(0b101001, c.get_u8());
    assert_eq!(168, d.sum.get_u8());
    assert_eq!(0, d.carry.get());
}
