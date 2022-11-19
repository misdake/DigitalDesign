#[test]
fn test_flatten_unflatten() {
    use crate::{add_naive, clear_all, flatten3, input_w, simulate, unflatten3};
    clear_all();

    let a = input_w::<2>();
    let b = input_w::<3>();
    let c = input_w::<3>();
    let d = input_w::<8>();
    a.set_u8(3);
    b.set_u8(0);
    c.set_u8(3);
    d.set_u8(46);
    let f = flatten3(a, b, c);
    assert_eq!(99, f.get_u8());
    let r = add_naive(d, f); // 3 + 3<<5 + 46 = 145
    simulate();
    assert_eq!(145, r.sum.get_u8());
    let (x, y, z) = unflatten3::<2, 3, 3>(f);
    assert_eq!(3, x.get_u8());
    assert_eq!(0, y.get_u8());
    assert_eq!(3, z.get_u8());
}
