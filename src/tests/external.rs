#[test]
fn test_external() {
    use crate::{clear_all, external, input, simulate, Logger};
    clear_all();

    let a = input();
    let b = input();
    let c = a | b;
    let logger = external(Logger::new("or".to_string(), c));
    for i in 0..10 {
        a.set(if i % 3 == 0 { 1 } else { 0 });
        b.set(if i % 2 == 0 { 1 } else { 0 });
        simulate();
    }
    assert_eq!(logger.get_values(), &vec![1, 0, 1, 1, 1, 0, 1, 0, 1, 1]);
}
