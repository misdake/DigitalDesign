//! rcc ports of the legacy embedded-DSL behavior tests (the embedded DSL is
//! deleted; these are the same scenarios written in rcc text form).

mod common;

use common::*;

#[test]
fn test_call_add() {
    let x = 12u16;
    let y = 43u16;
    let src = r#"
fn add(a: u16, b: u16) -> u16 {
    a + b
}
fn main() {
    halt(add(12, 43));
}
"#;
    assert_eq!(run(src), Some(x + y));
}

#[test]
fn test_for_loop() {
    let src = r#"
fn main() {
    let mut sum: u16 = 0;
    for i in 1..=10u16 {
        sum += i;
    }
    halt(sum);
}
"#;
    assert_eq!(run(src), Some((1..=10u16).sum()));
}

#[test]
fn test_for_loop2() {
    let src = r#"
fn main() {
    let start = 1;
    let end = 6;

    let mut sum = 0;
    let mut r1 = 0;
    for i in start..end {
        sum += i;
        if sum <= 6 {
            r1 = i;
        }
    }

    let mut sum = 0;
    let mut r2 = 0;
    let mut i = end - 1;
    while i >= start {
        sum += i;
        if sum < 6 {
            r2 = i;
        }
        i -= 1;
    }

    halt((r1 << 4) + r2);
}
"#;
    // up: 1 + 2 + 3, r1 = 3; rev: 5, r2 = 5
    assert_eq!(run(src), Some((3 << 4) + 5));
}

#[test]
fn test_ptr_array() {
    let src = r#"
fn main() {
    let mut sum: u16 = 0;
    for i in 0..8u16 {
        Ptr::from_addr(i).write(0, 11);
    }
    let array1 = Ptr::from_addr(8);
    let array2 = Ptr::from_addr(9);
    let mut j = 0;
    while j < 4 {
        array1.add((j << 1) as i16).write(0, 4);
        array2.add((j << 1) as i16).write(0, 7);
        j += 1;
    }
    for i in 0..8u16 {
        sum += Ptr::from_addr(i).read(0);
    }
    let mut j = 0;
    while j < 4 {
        sum += array1.add((j << 1) as i16).read(0);
        sum += array2.add((j << 1) as i16).read(0);
        j += 1;
    }
    halt(sum);
}
"#;
    // 11*8 + 4*4 + 7*4 = 132
    assert_eq!(run(src), Some(11 * 12));
}

#[test]
fn test_struct() {
    // a Vec2 { x, y } at 555+2, fields are pointer offsets (no struct sugar)
    let src = r#"
fn main() {
    let base = Ptr::from_addr(555);
    let vec2 = base.add(2);
    vec2.write(0, 123); // .x
    vec2.write(1, 456); // .y
    halt(vec2.read(0));
}
"#;
    let (state, signal) = compile_and_run(src, "main", 1000);
    assert_eq!(signal, Some(123));
    assert_eq!(state.mem[555], 0);
    assert_eq!(state.mem[556], 0);
    assert_eq!(state.mem[557], 123);
    assert_eq!(state.mem[558], 456);
    assert_eq!(state.mem[559], 0);
}

#[test]
fn test_bool_combinators() {
    let src = r#"
fn clamp(x: u16) -> u16 {
    if x >= 2 && x <= 10 {
        x
    } else {
        0
    }
}
fn main() {
    let r1 = clamp(5);
    let r2 = clamp(20);
    halt((r1 << 4) + r2);
}
"#;
    assert_eq!(run(src), Some(5 << 4));
}

#[test]
fn test_while_loop_break() {
    let src = r#"
fn main() {
    let mut v: u16 = 27;
    let mut steps: u16 = 0;
    while v != 1 && steps < 200 {
        let half = v >> 1;
        let doubled = half << 1;
        if doubled == v {
            v = half;
        } else {
            v = (v << 1) + v + 1;
        }
        steps += 1;
    }
    halt(steps);
}
"#;
    let (mut v, mut steps) = (27u16, 0u16);
    while v != 1 && steps < 200 {
        v = if v % 2 == 0 { v / 2 } else { 3 * v + 1 };
        steps += 1;
    }
    assert_eq!(run(src), Some(steps));
}
