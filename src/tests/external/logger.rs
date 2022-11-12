#[test]
fn test_logger() {
    use crate::*;
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
#[test]
fn test_logger_u8() {
    use crate::*;
    clear_all();

    let one = input_w::<4>();
    one.set_u8(1);
    let mut curr = reg_w::<4>();
    curr.set_in(add_naive(&curr.out, &one).sum);
    let logger = external(LoggerU8::new("inc".to_string(), curr.clone().out));
    for _ in 0..=16 {
        simulate();
    }
    assert_eq!(
        logger.get_values(),
        &vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 0]
    );
}
