//! rcc control flow: if/else-if/else, if-expressions, boolean combinators,
//! while/loop/break/continue, for loops over ranges, nesting.

mod common;

use common::*;

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
    assert_eq!(run(src), Some(39));
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
    assert_eq!(run(src), Some(sum));
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
    let a = clamp(5);
    let b = clamp(20);
    halt((a << 4) + b);
}
"#;
    assert_eq!(run(src), Some(5 << 4));
}

// ---------------------------------------------------------------------------
// deeper control-flow coverage
// ---------------------------------------------------------------------------

#[test]
fn test_if_else_if_chain() {
    let src = r#"
fn classify(x: u16) -> u16 {
    let mut r: u16 = 40;
    if x < 3 {
        r = 10;
    } else if x < 6 {
        r = 20;
    } else if x < 9 {
        r = 30;
    }
    r
}
fn grade(x: u16) -> u16 {
    // same chain through early returns
    if x < 3 {
        return 10;
    }
    if x < 6 {
        return 20;
    }
    if x < 9 {
        return 30;
    }
    40
}
fn main() {
    let mut acc: u16 = 0;
    let mut x: u16 = 0;
    while x <= 10 {
        acc += classify(x) + grade(x);
        x += 1;
    }
    halt(acc);
}
"#;
    let classify = |x: u16| -> u16 {
        if x < 3 {
            10
        } else if x < 6 {
            20
        } else if x < 9 {
            30
        } else {
            40
        }
    };
    let expect: u16 = (0..=10u16).map(|x| classify(x) + classify(x)).sum();
    assert_eq!(run(src), Some(expect));
}

#[test]
fn test_if_expression_forms() {
    let src = r#"
fn sign(x: i16) -> i16 {
    if x < 0 { -1 } else { 1 } // tail-position if-expression
}
fn cap(x: u16) -> u16 {
    // untyped literal branch unifies with the u16 branch
    let y: u16 = if x > 10 { 10 } else { x };
    y
}
fn deep(a: u16, b: u16, c: u16) -> u16 {
    // nested if-expression as a branch value
    if a > b {
        if b > c { 1 } else { 2 }
    } else {
        3
    }
}
fn choose(f: u16, x: u16) -> u16 {
    // call-valued branches
    if f == 0 { cap(x) } else { sign(x as i16) as u16 }
}
fn main() {
    let a = (sign(-7) + sign(3)) as u16;
    let b = cap(5) + cap(99);
    let c = deep(5, 3, 1) + deep(5, 3, 9) + deep(1, 3, 1);
    let d = choose(0, 7) + choose(1, 4);
    halt(a + b + c + d);
}
"#;
    let sign = |x: i16| -> i16 {
        if x < 0 {
            -1
        } else {
            1
        }
    };
    let cap = |x: u16| -> u16 {
        if x > 10 {
            10
        } else {
            x
        }
    };
    let deep = |a: u16, b: u16, c: u16| -> u16 {
        if a > b {
            if b > c {
                1
            } else {
                2
            }
        } else {
            3
        }
    };
    let choose = |f: u16, x: u16| -> u16 {
        if f == 0 {
            cap(x)
        } else {
            sign(x as i16) as u16
        }
    };
    let expect = (sign(-7) + sign(3)) as u16
        + cap(5)
        + cap(99)
        + deep(5, 3, 1)
        + deep(5, 3, 9)
        + deep(1, 3, 1)
        + choose(0, 7)
        + choose(1, 4);
    assert_eq!(run(src), Some(expect));
}

#[test]
fn test_if_expression_type_errors() {
    // both branches must have the same type (untyped literals unify, concrete ints do not)
    expect_error(
        "fn f(x: u16) -> u16 { let y = if x > 0 { 1i16 } else { 2u16 }; x }",
        "if-expression branches have different types",
    );
    expect_error(
        "fn f(x: u16) -> u16 { let p = Ptr::from_addr(0); let y = if x > 0 { p } else { 1u16 }; x }",
        "if-expression branches have different types",
    );
    // an if-expression always needs an else branch
    expect_error(
        "fn f(x: u16) -> u16 { let y = if x > 0 { 1u16 }; x }",
        "if-expression needs an else branch",
    );
    // else-if chains are statement-only; the expression form wants blocks
    expect_error(
        "fn f(x: u16) -> u16 { let y = if x > 0 { 1 } else if x > 1 { 2 } else { 3 }; x }",
        "if-expression branches must be blocks",
    );
}

#[test]
fn test_bool_nested_ops_truth_table() {
    // pack the truth table of two nested bool formulas over (x,y,z) in {0,1}^3
    // into one 16-bit word (2 bits per row, 8 rows)
    let src = r#"
fn main() {
    let mut acc: u16 = 0;
    let mut n: u16 = 0;
    while n < 8 {
        let x = (n >> 2) & 1;
        let y = (n >> 1) & 1;
        let z = n & 1;
        let f1 = if x == 1 && (y == 1 || z == 1) { 1 } else { 0 };
        let f2 = if !(x == 1 || y == 1) && z == 1 { 1 } else { 0 };
        acc = (acc << 2) + (f1 << 1) + f2;
        n += 1;
    }
    halt(acc);
}
"#;
    let mut acc = 0u16;
    let mut n = 0u16;
    while n < 8 {
        let x = (n >> 2) & 1;
        let y = (n >> 1) & 1;
        let z = n & 1;
        let f1 = if x == 1 && (y == 1 || z == 1) { 1 } else { 0 };
        let f2 = if !(x == 1 || y == 1) && z == 1 { 1 } else { 0 };
        acc = (acc << 2) + (f1 << 1) + f2;
        n += 1;
    }
    assert_eq!(run(src), Some(acc));
}

#[test]
fn test_bool_not_de_morgan() {
    // !(a && b) must behave like !a || !b on every input pair
    let src = r#"
fn main() {
    let mut nand_pat: u16 = 0;
    let mut or_pat: u16 = 0;
    let mut n: u16 = 0;
    while n < 4 {
        let x = (n >> 1) & 1;
        let y = n & 1;
        let a = if !(x == 1 && y == 1) { 1 } else { 0 };
        #[allow(clippy::nonminimal_bool)]
        let b = if !(x == 1) || !(y == 1) { 1 } else { 0 };
        nand_pat = (nand_pat << 1) + a;
        or_pat = (or_pat << 1) + b;
        n += 1;
    }
    halt((nand_pat << 4) + or_pat);
}
"#;
    let (mut nand_pat, mut or_pat, mut n) = (0u16, 0u16, 0u16);
    while n < 4 {
        let x = (n >> 1) & 1;
        let y = n & 1;
        let a = if !(x == 1 && y == 1) { 1 } else { 0 };
        #[allow(clippy::nonminimal_bool)]
        let b = if !(x == 1) || !(y == 1) { 1 } else { 0 };
        nand_pat = (nand_pat << 1) + a;
        or_pat = (or_pat << 1) + b;
        n += 1;
    }
    assert_eq!(run(src), Some((nand_pat << 4) + or_pat));
}

#[test]
fn test_bool_rhs_evaluated_left_to_right() {
    // when the lhs does NOT decide the result, the rhs must be evaluated,
    // exactly once and after the lhs (observed via a call counter)
    let src = r#"
static CALLS: u16 = 0;

fn bump() -> u16 {
    let c = CALLS + 1;
    addr_of(&CALLS).write(0, c);
    c
}

fn main() {
    let mut hits: u16 = 0;
    // lhs true for &&: rhs runs (first call returns 1)
    if 1u16 == 1 && bump() == 1 {
        hits += 1;
    }
    // lhs false for ||: rhs runs (second call returns 2)
    if 1u16 == 2 || bump() == 2 {
        hits += 1;
    }
    // chained &&: third and fourth calls return 3 and 4
    if bump() == 3 && bump() == 4 {
        hits += 1;
    }
    halt((hits << 8) + CALLS);
}
"#;
    let (mut hits, mut calls) = (0u16, 0u16);
    let mut bump = || {
        calls += 1;
        calls
    };
    if 1u16 == 1 && bump() == 1 {
        hits += 1;
    }
    if 1u16 == 2 || bump() == 2 {
        hits += 1;
    }
    if bump() == 3 && bump() == 4 {
        hits += 1;
    }
    assert_eq!(run(src), Some((hits << 8) + calls));
}

#[test]
fn test_bool_short_circuit_skips_rhs() {
    let src = r#"
static CALLS: u16 = 0;

fn bump() -> u16 {
    let c = CALLS + 1;
    addr_of(&CALLS).write(0, c);
    c
}

fn main() {
    // lhs false decides &&: bump must not run
    if 1u16 == 2 && bump() > 0 {
        halt(90);
    }
    // lhs true decides ||: bump must not run
    if 1u16 == 1 || bump() > 0 {
    } else {
        halt(91);
    }
    halt(CALLS);
}
"#;
    // Rust semantics: bump is never called -> CALLS == 0
    // (the current compiler runs bump twice -> CALLS == 2)
    assert_eq!(run(src), Some(0));
}

#[test]
fn test_while_loop_carried_fib() {
    // several loop-carried variables (a, b, sum, i) all become phis
    let src = r#"
fn main() {
    let mut a: u16 = 0;
    let mut b: u16 = 1;
    let mut sum: u16 = 0;
    let mut i: u16 = 0;
    while i < 20 {
        let t = a + b;
        a = b;
        b = t;
        sum += a;
        i += 1;
    }
    halt(sum + b);
}
"#;
    let (mut a, mut b, mut sum) = (0u16, 1u16, 0u16);
    let mut i = 0;
    while i < 20 {
        let t = a + b;
        a = b;
        b = t;
        sum += a;
        i += 1;
    }
    assert_eq!(run(src), Some(sum + b));
}

#[test]
fn test_loop_break_and_continue() {
    let src = r#"
fn main() {
    let mut i: u16 = 0;
    let mut sum: u16 = 0;
    loop {
        i += 1;
        if i == 4 {
            continue;
        }
        sum += i;
        if i >= 6 {
            break;
        }
    }
    halt(sum);
}
"#;
    let (mut i, mut sum) = (0u16, 0u16);
    loop {
        i += 1;
        if i == 4 {
            continue;
        }
        sum += i;
        if i >= 6 {
            break;
        }
    }
    assert_eq!(run(src), Some(sum));
}

#[test]
fn test_nested_loops_inner_break() {
    let src = r#"
fn main() {
    let mut total: u16 = 0;
    for i in 0..5u16 {
        for j in 0..5u16 {
            if j > i {
                break;
            }
            total += 1;
        }
    }
    halt(total);
}
"#;
    let mut total = 0u16;
    for i in 0..5u16 {
        for j in 0..5u16 {
            if j > i {
                break;
            }
            total += 1;
        }
    }
    assert_eq!(run(src), Some(total));
}

#[test]
fn test_for_ranges() {
    let src = r#"
fn sum_excl(from: u16, to: u16) -> u16 {
    let mut s: u16 = 0;
    for i in from..to {
        s += i;
    }
    s
}
fn sum_incl(from: u16, to: u16) -> u16 {
    let mut s: u16 = 0;
    for i in from..=to {
        s += i;
    }
    s
}
fn main() {
    let mut total: u16 = 0;
    total += sum_excl(0, 10); // 0+1+..+9
    total += sum_excl(5, 5); // empty
    total += sum_excl(5, 3); // empty (start > end)
    total += sum_incl(3, 3); // single element
    total += sum_incl(3, 2); // empty inclusive
    total += sum_incl(0, 10); // 0+1+..+10
    // plain unsuffixed literal range
    let mut n: u16 = 0;
    for i in 0..10 {
        n += 1;
    }
    total += n;
    halt(total);
}
"#;
    let excl = |from: u16, to: u16| -> u16 { (from..to).sum() };
    let incl = |from: u16, to: u16| -> u16 { (from..=to).sum() };
    let expect = excl(0, 10)
        + excl(5, 5)
        + excl(5, 3)
        + incl(3, 3)
        + incl(3, 2)
        + incl(0, 10)
        + (0..10).count() as u16;
    assert_eq!(run(src), Some(expect));
}

#[test]
fn test_for_i16_range_vars() {
    // signed range variables: crossing zero and fully negative
    let src = r#"
fn main() {
    let mut s1: i16 = 0;
    let mut c1: u16 = 0;
    for i in -5i16..10i16 {
        s1 += i;
        c1 += 1;
    }
    let mut s2: i16 = 0;
    for i in -10i16..-5i16 {
        s2 += i;
    }
    let total = ((s1 + s2 + 100i16) as u16) + (c1 << 8);
    halt(total);
}
"#;
    let (mut s1, mut c1) = (0i16, 0u16);
    for i in -5i16..10i16 {
        s1 += i;
        c1 += 1;
    }
    let mut s2 = 0i16;
    for i in -10i16..-5i16 {
        s2 += i;
    }
    let expect = ((s1 + s2 + 100i16) as u16) + (c1 << 8);
    assert_eq!(run(src), Some(expect));
}

#[test]
fn test_for_range_type_errors() {
    expect_error("fn f() { for i in 0u16..10i16 { } }", "range type mismatch");
}

#[test]
fn test_break_continue_in_nested_if() {
    // break/continue buried under two levels of if, with else branches
    let src = r#"
fn main() {
    let mut i: u16 = 0;
    let mut sum: u16 = 0;
    let mut skips: u16 = 0;
    while i < 30 {
        i += 1;
        if i > 3 {
            if i > 25 {
                break;
            }
            if (i & 1) == 0 {
                sum += i;
            } else {
                skips += 1;
                continue;
            }
            if i == 10 {
                sum += 100;
            }
        } else {
            sum += 1;
        }
    }
    // break inside nested ifs of a for loop
    let mut ftotal: u16 = 0;
    for i in 0..100u16 {
        if i > 2 {
            if i + i > 20 {
                break;
            }
            ftotal += i;
        }
    }
    halt(sum + skips + ftotal);
}
"#;
    let (mut i, mut sum, mut skips) = (0u16, 0u16, 0u16);
    while i < 30 {
        i += 1;
        if i > 3 {
            if i > 25 {
                break;
            }
            if (i & 1) == 0 {
                sum += i;
            } else {
                skips += 1;
                continue;
            }
            if i == 10 {
                sum += 100;
            }
        } else {
            sum += 1;
        }
    }
    let mut ftotal = 0u16;
    for i in 0..100u16 {
        if i > 2 {
            if i + i > 20 {
                break;
            }
            ftotal += i;
        }
    }
    assert_eq!(run(src), Some(sum + skips + ftotal));
}

#[test]
fn test_for_continue_advances() {
    let src = r#"
fn main() {
    let mut sum: u16 = 0;
    for i in 0..10u16 {
        if i == 3 {
            continue;
        }
        sum += i;
    }
    halt(sum);
}
"#;
    // Rust semantics: sum of 0..10 minus 3 = 42
    // (the current compiler never reaches halt -> cycle cap -> None)
    let expect: u16 = (0..10u16).filter(|&i| i != 3).sum();
    assert_eq!(run(src), Some(expect));
}

#[test]
fn test_for_inclusive_max_boundary() {
    let src = r#"
fn main() {
    let mut n: u16 = 0;
    for i in 65534..=65535u16 {
        n += 1;
    }
    halt(n);
}
"#;
    assert_eq!(run(src), Some((65534..=65535u16).count() as u16));
}

#[test]
fn test_nested_grid_heavy() {
    // 100x100 grid: needs a bigger cycle budget than the default 10k
    let src = r#"
fn main() {
    let mut count: u16 = 0;
    let mut diag: u16 = 0;
    for i in 0..100u16 {
        for j in 0..100u16 {
            if ((i + j) & 1) == 0 {
                count += 1;
            }
            if i == j {
                diag += 1;
            }
        }
    }
    halt(count + diag);
}
"#;
    let (mut count, mut diag) = (0u16, 0u16);
    for i in 0..100u16 {
        for j in 0..100u16 {
            if ((i + j) & 1) == 0 {
                count += 1;
            }
            if i == j {
                diag += 1;
            }
        }
    }
    let (_state, signal) = compile_and_run(src, "main", 500_000);
    assert_eq!(signal, Some(count + diag));
}
