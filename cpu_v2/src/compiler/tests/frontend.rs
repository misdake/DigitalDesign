//! end-to-end tests for the rcc frontend (spec: compiler/frontend/spec.md).
//! the `*_dsl.rs` files under src/dsl_progs/ are both valid host Rust modules
//! (rust-analyzer reads them; rustc type-checks them at build time) and the
//! inputs compiled here for the target.

use crate::compiler::frontend::parse_source;
use crate::compiler::Compiler;
use crate::simulate;

fn compile_and_run(src: &str, main: &'static str, max_cycles: usize) -> Option<u16> {
    let funcs = parse_source(src).expect("parse failed");
    let mut c = Compiler::new();
    for f in funcs {
        c.add_func(f);
    }
    let (instructions, listing) = c.finish(main);
    println!("{listing}");
    let (_state, signal) = simulate(&instructions, max_cycles);
    signal
}

#[test]
fn test_sum_dsl() {
    let src = include_str!("../../dsl_progs/sum_dsl.rs");
    let signal = compile_and_run(src, "main", 1000);
    assert_eq!(signal, Some((1..=10u16).sum()));
}

#[test]
fn test_fnptr_dsl() {
    let src = include_str!("../../dsl_progs/fnptr_dsl.rs");
    let signal = compile_and_run(src, "main", 2000);
    assert_eq!(signal, Some(42));
}

#[test]
fn test_signed_ops() {
    let src = r#"
fn clamp(x: i16) -> i16 {
    if x >= 2 && x <= 10 {
        x
    } else {
        0
    }
}
fn main() {
    let a = clamp(7);
    let b = clamp(-3);
    let d = (-8i16) >> 1;
    let e = d + a + b;
    halt(e as u16);
}
"#;
    let signal = compile_and_run(src, "main", 1000);
    // clamp(7)=7, clamp(-3)=0, (-8)>>1 = -4 (arithmetic), sum = 7+0-4 = 3
    assert_eq!(signal, Some(3));
}

#[test]
fn test_if_expression() {
    let src = r#"
fn max(a: u16, b: u16) -> u16 {
    let m = if a > b { a } else { b };
    m
}
fn main() {
    let x = max(30, 20);
    let y = max(5, 9);
    halt(x + y);
}
"#;
    let signal = compile_and_run(src, "main", 1000);
    assert_eq!(signal, Some(39));
}

#[test]
fn test_while_break_continue() {
    let src = r#"
fn main() {
    let mut i: u16 = 0;
    let mut sum: u16 = 0;
    while i < 20 {
        i += 2;
        if i == 6 {
            continue;
        }
        if sum > 30 {
            break;
        }
        sum += i;
    }
    halt(sum);
}
"#;
    let signal = compile_and_run(src, "main", 2000);
    // rust reference
    let (mut sum, mut i) = (0u16, 0u16);
    while i < 20 {
        i += 2;
        if i == 6 {
            continue;
        }
        if sum > 30 {
            break;
        }
        sum += i;
    }
    assert_eq!(signal, Some(sum));
}

#[test]
fn test_arrays_dsl() {
    let src = include_str!("../../dsl_progs/arrays_dsl.rs");
    let signal = compile_and_run(src, "main", 4000);
    // TILE[1] = 0xc3ff lands in grid[16], then in total, then in SCORE
    assert_eq!(signal, Some(0xc3ff));
}

#[test]
fn test_const_and_static() {
    let src = r#"
const WIDTH: u16 = 8;
const DOUBLE: u16 = WIDTH * 2;
static SCORE: u16 = 0;
static TILE: [u16; 3] = [11, 22, 33];
fn main() {
    // statics are initialized by __data_init at main entry
    assert(TILE.read(0) == 11 && TILE.read(2) == 33, 1);
    assert(DOUBLE == 16, 2);
    // writing through the address of a global
    let s = addr_of(&SCORE);
    s.write(0, TILE.read(1));
    halt(SCORE);
}
"#;
    let signal = compile_and_run(src, "main", 4000);
    assert_eq!(signal, Some(22));
}

#[test]
fn test_addr_of_local() {
    let src = r#"
fn bump(p: Ptr) {
    let v = p.read(0);
    p.write(0, v + 1);
}
fn main() {
    let mut x: u16 = 41;
    bump(addr_of(&x));
    bump(addr_of(&x));
    halt(x);
}
"#;
    let signal = compile_and_run(src, "main", 2000);
    assert_eq!(signal, Some(43));
}

#[test]
fn test_local_array_as_param() {
    let src = r#"
fn fill(buf: Ptr, n: u16, v: u16) {
    let mut i: u16 = 0;
    while i < n {
        buf.add(i as i16).write(0, v);
        i += 1;
    }
}
fn sum(buf: Ptr, n: u16) -> u16 {
    let mut s: u16 = 0;
    let mut i: u16 = 0;
    while i < n {
        s += buf.add(i as i16).read(0);
        i += 1;
    }
    s
}
fn main() {
    let mut a: [u16; 10] = [0; 10];
    fill(a.as_ptr(), 10, 4);
    halt(sum(a.as_ptr(), 10));
}
"#;
    let signal = compile_and_run(src, "main", 4000);
    assert_eq!(signal, Some(40));
}

#[test]
fn test_addr_of_param() {
    let src = r#"
fn set1(p: Ptr) {
    p.write(0, 1);
}
fn choose(x: u16, y: u16) -> u16 {
    let mut z: u16 = x;
    if y != 0 {
        set1(addr_of(&z));
    }
    z
}
fn main() {
    let a = choose(7, 1);
    let b = choose(7, 0);
    halt((a << 4) + b);
}
"#;
    let signal = compile_and_run(src, "main", 2000);
    assert_eq!(signal, Some((1 << 4) + 7));
}

// ---------------------------------------------------------------------------
// subset violations must be hard errors
// ---------------------------------------------------------------------------

fn expect_error(src: &str, needle: &str) {
    match parse_source(src) {
        Ok(_) => panic!("expected error containing `{needle}`, got Ok"),
        Err(e) => {
            let msg = e.to_string();
            assert!(msg.contains(needle), "error `{msg}` should contain `{needle}`");
            println!("ok: {e}");
        }
    }
}

#[test]
fn test_unsupported_constructs() {
    expect_error("fn f(x: u16) -> u16 { x / 2 }", "not supported");
    expect_error("fn f(x: u16) -> u16 { x * 2 }", "not supported");
    expect_error("fn f(x: u16) -> u16 { match x { _ => 0 } }", "not supported");
    expect_error("fn f(x: u16) -> u16 { let g = |y| y; x }", "not supported");
    expect_error("fn f<T>(x: T) -> T { x }", "not supported");
    expect_error("fn f(x: u16) -> u16 { x as u32 }", "not supported");
    expect_error("struct S { x: u16 }", "not supported");
    expect_error("fn f(x: u16) -> u16 { let y = x + 1i16; y }", "mismatch");
    expect_error("fn f(x: u16) { let b = x < 3u16; }", "bool");
    expect_error("fn f(x: u16) { if x { halt(0); } }", "boolean");
    expect_error("fn f(x: u16) { x = 1; }", "not mutable");
    expect_error("fn f(x: u16) -> u16 { return; }", "return");
    expect_error("fn f(x: u16) -> u32 { x }", "not supported");
    expect_error("fn f() { let a: [u16; 3]; }", "initializer");
    expect_error("fn f() { let mut x: u16 = 1; let p = &x; }", "not supported");
    expect_error("static mut X: u16 = 0; fn f() {}", "static mut");
    expect_error("fn f(a: [u16; 2]) {}", "array");
}
