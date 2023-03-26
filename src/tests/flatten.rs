#[test]
fn test_flatten_unflatten() {
    use crate::{add_naive, clear_all, flatten3, input_w, simulate, unflatten3};
    clear_all();

    let a = input_w::<2>();
    let b = input_w::<3>();
    let c = input_w::<3>();
    let d = input_w::<8>();
    a.set_u8(1);
    b.set_u8(2);
    c.set_u8(3);
    d.set_u8(46);
    let f = flatten3(a, b, c);
    assert_eq!(105, f.get_u8()); // 1 + 2<<2 + 3<<5
    let r = add_naive(d, f); // 105 + 46 = 151
    simulate();
    assert_eq!(151, r.sum.get_u8());
    let (x, y, z) = unflatten3::<2, 3, 3>(f);
    assert_eq!(1, x.get_u8());
    assert_eq!(2, y.get_u8());
    assert_eq!(3, z.get_u8());

    d.set_u8(0b11010010);
    let (x, y, z) = unflatten3::<2, 2, 4>(d);
    assert_eq!(0b10, x.get_u8());
    assert_eq!(0b00, y.get_u8());
    assert_eq!(0b1101, z.get_u8());
}
