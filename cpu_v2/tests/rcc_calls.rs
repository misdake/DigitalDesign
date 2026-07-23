//! rcc calls: direct calls, function pointers (bind/pass/call), intrinsics
//! (halt, assert), recursion.

mod common;

use common::*;

#[test]
fn test_fnptr_dsl_file() {
    let src = include_str!("../src/dsl_progs/fnptr_dsl.rs");
    let (_state, signal) = compile_and_run(src, "main", 2000);
    assert_eq!(signal, Some(42));
}

#[test]
fn test_recursion() {
    let src = r#"
fn fib(n: u16) -> u16 {
    if n < 2 {
        n
    } else {
        fib(n - 1) + fib(n - 2)
    }
}
fn main() {
    halt(fib(10));
}
"#;
    assert_eq!(run(src), Some(55));
}

#[test]
fn test_fnptr_as_argument() {
    let src = r#"
fn inc(x: u16) -> u16 { x + 1 }
fn dec(x: u16) -> u16 { x - 1 }
fn twice(f: fn(u16) -> u16, x: u16) -> u16 {
    f(f(x))
}
fn main() {
    let a = twice(inc, 10);
    let g: fn(u16) -> u16 = dec;
    let b = twice(g, 10);
    halt((a << 4) + b);
}
"#;
    assert_eq!(run(src), Some((12 << 4) + 8));
}

// ---------------------------------------------------------------------------
// direct calls
// ---------------------------------------------------------------------------

#[test]
fn test_direct_call_six_args() {
    let src = r#"
fn add6(a: u16, b: u16, c: u16, d: u16, e: u16, f: u16) -> u16 {
    a + b + c + d + e + f
}
fn main() {
    let r = add6(1, 2, 3, 4, 5, 6);
    // a nested call as an argument: all 6 ABI slots in flight at once
    let s = add6(add6(1, 1, 1, 1, 1, 1), 2, 3, 4, 5, 6);
    halt(r + s);
}
"#;
    let add6 = |a: u16, b: u16, c: u16, d: u16, e: u16, f: u16| a + b + c + d + e + f;
    let expected = add6(1, 2, 3, 4, 5, 6) + add6(add6(1, 1, 1, 1, 1, 1), 2, 3, 4, 5, 6);
    assert_eq!(run(src), Some(expected));
}

#[test]
fn test_direct_call_chain_mixed_types() {
    let src = r#"
fn neg(x: i16) -> i16 { -x }
fn abs2(x: i16) -> u16 {
    // |x| * 2 without a multiply: pick x or -x, then double it
    let a = if x < 0 { neg(x) } else { x };
    (a + a) as u16
}
fn main() {
    let a = abs2(-7);
    let b = abs2(3);
    halt(a + b);
}
"#;
    let abs2 = |x: i16| (x as i32).unsigned_abs() as u16 * 2;
    assert_eq!(run(src), Some(abs2(-7) + abs2(3)));
}

// ---------------------------------------------------------------------------
// recursion (frame-heavy: raised cycle caps)
// ---------------------------------------------------------------------------

#[test]
fn test_recursion_fib_deep() {
    let src = r#"
fn fib(n: u16) -> u16 {
    if n < 2 {
        n
    } else {
        fib(n - 1) + fib(n - 2)
    }
}
fn main() {
    halt(fib(15));
}
"#;
    fn fib(n: u16) -> u16 {
        if n < 2 {
            n
        } else {
            fib(n - 1) + fib(n - 2)
        }
    }
    // fib(15) makes ~2000 nested calls; frame setup needs a generous cap
    let (_state, signal) = compile_and_run(src, "main", 500_000);
    assert_eq!(signal, Some(fib(15)));
}

#[test]
fn test_recursion_fact() {
    let src = r#"
fn mul_add(a: u16, b: u16) -> u16 {
    // a * b by repeated addition (the ISA has no multiply)
    let mut acc: u16 = 0;
    let mut i: u16 = 0;
    while i < b {
        acc += a;
        i += 1;
    }
    acc
}
fn fact(n: u16) -> u16 {
    if n < 2 {
        1
    } else {
        mul_add(n, fact(n - 1))
    }
}
fn main() {
    halt(fact(6));
}
"#;
    let expected: u16 = (1..=6).product();
    let (_state, signal) = compile_and_run(src, "main", 100_000);
    assert_eq!(signal, Some(expected));
}

#[test]
fn test_recursion_deep_linear() {
    let src = r#"
fn sum_to(n: u16) -> u16 {
    if n == 0 {
        0
    } else {
        n + sum_to(n - 1)
    }
}
fn main() {
    halt(sum_to(200));
}
"#;
    // 200 live frames at the deepest point; the stack wraps down from
    // address 0 into the top of data memory, so this fits easily
    let expected: u16 = (1..=200).sum();
    let (_state, signal) = compile_and_run(src, "main", 200_000);
    assert_eq!(signal, Some(expected));
}

#[test]
fn test_mutual_recursion_any_order() {
    // main first, and is_even calls is_odd before it is defined:
    // signature collection is order-independent
    let src = r#"
fn main() {
    halt(is_even(10) + is_odd(7));
}
fn is_even(n: u16) -> u16 {
    if n == 0 {
        1
    } else {
        is_odd(n - 1)
    }
}
fn is_odd(n: u16) -> u16 {
    if n == 0 {
        0
    } else {
        is_even(n - 1)
    }
}
"#;
    fn is_even(n: u16) -> u16 {
        if n == 0 {
            1
        } else {
            is_odd(n - 1)
        }
    }
    fn is_odd(n: u16) -> u16 {
        if n == 0 {
            0
        } else {
            is_even(n - 1)
        }
    }
    let (_state, signal) = compile_and_run(src, "main", 100_000);
    assert_eq!(signal, Some(is_even(10) + is_odd(7)));
}

// ---------------------------------------------------------------------------
// function pointers
// ---------------------------------------------------------------------------

#[test]
fn test_fnptr_bind_and_call() {
    let src = r#"
fn add1(x: u16) -> u16 { x + 1 }
fn add2(x: u16) -> u16 { x + 2 }
fn main() {
    let f: fn(u16) -> u16 = add1;
    let g: fn(u16) -> u16 = add2;
    // both calls go through call_reg
    halt((f(10) << 4) + g(10));
}
"#;
    assert_eq!(run(src), Some(((10 + 1) << 4) + (10 + 2)));
}

#[test]
fn test_fnptr_relayed_as_argument() {
    let src = r#"
fn inc(x: u16) -> u16 { x + 1 }
fn apply(f: fn(u16) -> u16, x: u16) -> u16 {
    f(x)
}
fn relay(f: fn(u16) -> u16, x: u16) -> u16 {
    // a fn pointer parameter passed onwards as a fn pointer argument
    apply(f, x) + apply(f, x + 10)
}
fn main() {
    let g: fn(u16) -> u16 = inc;
    // bound variable in one call, inline function item in the other
    halt(relay(g, 1) + relay(inc, 100));
}
"#;
    let relay = |x: u16| (x + 1) + (x + 10 + 1);
    assert_eq!(run(src), Some(relay(1) + relay(100)));
}

#[test]
fn test_fnptr_indirect_multi_arg() {
    let src = r#"
fn pack(a: u16, b: u16, c: u16, d: u16) -> u16 {
    (a << 8) + (b << 4) + c + d
}
fn apply4(f: fn(u16, u16, u16, u16) -> u16, a: u16, b: u16, c: u16, d: u16) -> u16 {
    f(a, b, c, d)
}
fn main() {
    let f: fn(u16, u16, u16, u16) -> u16 = pack;
    let via_var = f(1, 2, 3, 4);
    let via_param = apply4(pack, 5, 6, 7, 8);
    halt(via_var + via_param);
}
"#;
    let pack = |a: u16, b: u16, c: u16, d: u16| (a << 8) + (b << 4) + c + d;
    assert_eq!(run(src), Some(pack(1, 2, 3, 4) + pack(5, 6, 7, 8)));
}

#[test]
fn test_fnptr_i16_indirect() {
    let src = r#"
fn distance(a: i16, b: i16) -> i16 {
    if a < b {
        b - a
    } else {
        a - b
    }
}
fn main() {
    let f: fn(i16, i16) -> i16 = distance;
    let d: u16 = f(-20, 5) as u16;
    halt(d);
}
"#;
    assert_eq!(run(src), Some((5i16 - -20) as u16));
}

#[test]
fn test_fnptr_signature_mismatch() {
    // wrong arity
    expect_error(
        r#"
fn double(x: u16) -> u16 { x + x }
fn main() {
    let g: fn(u16, u16) -> u16 = double;
    halt(g(1, 2));
}
"#,
        "fn pointer type mismatch",
    );
    // wrong return type
    expect_error(
        r#"
fn double(x: u16) -> u16 { x + x }
fn main() {
    let g: fn(u16) = double;
    g(1);
}
"#,
        "fn pointer type mismatch",
    );
    // wrong argument type
    expect_error(
        r#"
fn double(x: u16) -> u16 { x + x }
fn main() {
    let g: fn(i16) -> u16 = double;
    halt(g(1));
}
"#,
        "fn pointer type mismatch",
    );
}

// ---------------------------------------------------------------------------
// intrinsics: halt / assert
// ---------------------------------------------------------------------------

#[test]
fn test_halt_signal_value() {
    let src = r#"
fn answer() -> u16 { 40 + 2 }
fn main() {
    let x = answer();
    halt(x);
}
"#;
    assert_eq!(run(src), Some(40 + 2));
}

#[test]
fn test_assert_holds_no_halt() {
    let src = r#"
fn checked_inc(x: u16) -> u16 {
    assert(x > 0, 0xE001);
    assert(x < 100, 0xE002);
    x + 1
}
fn main() {
    let r = checked_inc(41);
    assert(r == 42, 0xE003);
    // every assert holds, so none of the 0xE00x signals may fire
    halt(r);
}
"#;
    assert_eq!(run(src), Some(41 + 1));
}

#[test]
fn test_assert_failure_signal() {
    // a failing assert halts with exactly its own signal
    let src = r#"
fn main() {
    let x: u16 = 3;
    assert(x > 10, 0x5A);
    halt(0xFF);
}
"#;
    assert_eq!(run(src), Some(0x5A));

    // same, but the failing assert sits inside a called function
    let src = r#"
fn must_be_small(x: u16) -> u16 {
    assert(x < 10, 0x77);
    x
}
fn main() {
    halt(must_be_small(25));
}
"#;
    assert_eq!(run(src), Some(0x77));
}
