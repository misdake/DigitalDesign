//! DSL frontend behavior tests: functions, calls, loops, pointers/arrays,
//! structs, boolean combinators, break/continue — all verified on the simulator.

use crate::dsl::*;
use crate::{define_struct, simulate, Compiler};

#[test]
fn test_call_add() {
    let x = 12u16;
    let y = 43u16;
    let call = DslFunction::new("call", [], []);
    let add = DslFunction::new("add", ["a", "b"], ["r"]);

    let mut compiler = Compiler::new();
    call.compile(&mut compiler, |b, [], _ret| {
        let a = b.v(x);
        let c = b.v(y);
        let [r] = add.call(b, [&a, &c]);
        b.halt(&r);
    });
    add.compile(&mut compiler, |b, [a, c], ret| {
        let r = &a + &c;
        ret(b, [r]);
    });

    let (instructions, _) = compiler.finish("call");
    let (_state, signal) = simulate(&instructions, 1000);
    assert_eq!(signal, Some(x + y));
}

#[test]
fn test_for_loop() {
    let n = 10u8;
    let func = DslFunction::new("loop", [], []);

    let mut compiler = Compiler::new();
    func.compile(&mut compiler, |b, [], _ret| {
        let mut sum = b.v(0);
        b.for_loop_u4(1..(n + 1), |_b, i| {
            sum += &i;
        });
        b.halt(&sum);
    });

    let (instructions, _) = compiler.finish("loop");
    let (_state, signal) = simulate(&instructions, 1000);
    let n = n as u16;
    assert_eq!(signal, Some(n * (n + 1) / 2));
}

#[test]
fn test_for_loop2() {
    let func = DslFunction::new("loop2", [], []);

    let mut compiler = Compiler::new();
    func.compile(&mut compiler, |b, [], _ret| {
        // 1..=5
        let start = b.v(1);
        let end = b.v(6);

        let mut sum = b.v(0);
        let r1 = b.v(0);
        b.for_loop(&start, &end, 1, |b, i| {
            sum += &i;
            b.if_then(sum.le_imm(6), |_b| {
                r1.assign_from(&i);
            });
        });

        let mut sum = b.v(0);
        let r2 = b.v(0);
        b.for_loop_rev(&start, &end, 1, |b, i| {
            sum += &i;
            b.if_then(sum.lt_imm(6), |_b| {
                r2.assign_from(&i);
            });
        });

        let sig = &r1.lsl(4) + &r2;
        b.halt(&sig);
    });

    let (instructions, _) = compiler.finish("loop2");
    let (_state, signal) = simulate(&instructions, 1000);
    // up: 1 + 2 + 3, r1 = 3
    // up_rev: 5, r2 = 5
    assert_eq!(signal, Some((3 << 4) + 5));
}

#[test]
fn test_ptr_array() {
    let func = DslFunction::new("ptr_array", [], []);

    let mut compiler = Compiler::new();
    func.compile(&mut compiler, |b, [], _ret| {
        let c = b.v(11);
        let d = b.v(4);
        let e = b.v(7);
        b.for_loop_u4(0..8, |_b, i| {
            i.ptr().write(&c);
        });
        let array1 = DslArray::<2>::new(b.v(8).ptr());
        let array2 = DslArray::<2>::new(b.v(9).ptr());
        for j in 0..4 {
            array1.index_imm(j).write(&d);
        }
        b.for_loop_u4(0..4, |_b, i| {
            array2.index_reg(&i).write(&e);
        });

        let mut sum = b.v(0);
        b.for_loop_u4(0..8, |_b, i| {
            let v = i.ptr().read();
            sum += &v;
        });
        b.for_loop_u4(0..4, |_b, i| {
            let v = array1.index_reg(&i).read();
            sum += &v;
        });
        for j in 0..4 {
            let v = array2.index_imm(j).read();
            sum += &v;
        }

        b.halt(&sum);
    });

    let (instructions, _) = compiler.finish("ptr_array");
    let (_state, signal) = simulate(&instructions, 1000);
    assert_eq!(signal, Some(11 * 12));
}

#[test]
fn test_struct() {
    define_struct!(Vec2 { x, y });

    let func = DslFunction::new("test_struct", [], []);
    let mut compiler = Compiler::new();
    func.compile(&mut compiler, |b, [], _ret| {
        let base = DslArray::<{ Vec2::SIZE }>::new(b.v(555).ptr());

        let vec2 = Vec2::new(base.index_imm(1));
        vec2.x.write(&b.v(123));
        vec2.y.write(&b.v(456));

        let value = vec2.read();
        b.halt(&value.x);
    });

    let (instructions, _) = compiler.finish("test_struct");
    let (state, signal) = simulate(&instructions, 1000);
    assert_eq!(signal, Some(123));
    assert_eq!(state.mem[555], 0);
    assert_eq!(state.mem[556], 0);
    assert_eq!(state.mem[557], 123);
    assert_eq!(state.mem[558], 456);
    assert_eq!(state.mem[559], 0);
}

#[test]
fn test_bool_combinators() {
    // clamp(x) = (x >= 2 && x <= 10) ? x : 0
    let clamp = DslFunction::new("clamp", ["x"], ["r"]);
    let main = DslFunction::new("main", [], []);
    let mut compiler = Compiler::new();
    clamp.compile(&mut compiler, |b, [x], ret| {
        b.if_else(
            x.ge_imm(2) & x.le_imm(10),
            |b| ret(b, [x.clone()]),
            |b| {
                let z = b.v(0);
                ret(b, [z]);
            },
        );
    });
    main.compile(&mut compiler, |b, [], _ret| {
        let five = b.v(5);
        let twenty = b.v(20);
        let [r1] = clamp.call(b, [&five]);
        let [r2] = clamp.call(b, [&twenty]);
        let sig = &r1.lsl(4) + &r2;
        b.halt(&sig);
    });

    let (instructions, _) = compiler.finish("main");
    let (_state, signal) = simulate(&instructions, 1000);
    assert_eq!(signal, Some(5 << 4));
}

#[test]
fn test_while_loop_break() {
    // collatz steps until value reaches 1, with a step cap
    let func = DslFunction::new("collatz", [], []);
    let mut compiler = Compiler::new();
    func.compile(&mut compiler, |b, [], _ret| {
        let v = b.v(27);
        let steps = b.v(0);
        b.while_loop(
            |_| v.ne_imm(1) & steps.lt_imm(200),
            |b| {
                // if v is even: v /= 2 else v = 3v + 1
                let half = v.lsr(1);
                let doubled = half.lsl(1);
                let is_even = doubled.eq(&v);
                b.if_else(
                    is_even,
                    |_| v.assign_from(&half),
                    |_| {
                        let triple = &(&v.lsl(1) + &v) + 1;
                        v.assign_from(&triple);
                    },
                );
                let next = &steps + 1;
                steps.assign_from(&next);
            },
        );
        b.halt(&steps);
    });

    let (instructions, _) = compiler.finish("collatz");
    let (_state, signal) = simulate(&instructions, 10000);
    // rust reference
    let (mut v, mut steps) = (27u16, 0u16);
    while v != 1 && steps < 200 {
        v = if v % 2 == 0 { v / 2 } else { 3 * v + 1 };
        steps += 1;
    }
    assert_eq!(signal, Some(steps));
}
