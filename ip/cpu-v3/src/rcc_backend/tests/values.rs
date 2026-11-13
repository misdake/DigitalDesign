use super::*;

#[test]
fn large_repeat_initializers_evaluate_once_and_preserve_the_tail() {
    for count in [1, 31, 32, 33, 35, 40, 64] {
        let source = format!("static CALLS:u16=0; #[inline(never)] fn value()->u16{{unsafe{{addr_of(&CALLS).write(0,CALLS+1);}}7}} struct Frame{{before:u16,data:Buf<u16,{count}>,after:u16}} fn main(){{let f:Frame=Frame{{before:11,data:Buf::new([value();{count}]),after:13}};let mut i:u16=0;let mut sum:u16=0;while i<{count}{{sum+=f.data[i];i+=1;}}halt(sum+f.before+f.after+CALLS);}}");
        assert_eq!(
            execute_capped(compile(&source, CompilerOptions::default()), 30_000).0,
            7 * count + 25,
            "count {count}"
        );
    }
}

/// C2 lowers the contiguous-range vector surface: construction, vector
/// arithmetic, scalar broadcast (VMULS), unary map and lane extraction all
/// run through the two-word VECTOR ISA on the emulator.
#[test]
fn vector_construct_arithmetic_broadcast_unary_and_lanes_run() {
    let source = r#"
            fn main() {
                let a = vec3::new(fix32::from_int(1), fix32::from_int(2), fix32::from_int(3));
                let b = vec3::new(fix32::from_int(4), fix32::from_int(5), fix32::from_int(6));
                let c = a + b;                     // 5, 7, 9
                let d = c * fix32::from_int(2);    // 10, 14, 18
                let e = -d;                        // -10, -14, -18
                let f = e.abs();                   // 10, 14, 18
                halt((f.x() + f.y() + f.z()).to_int() as u16); // 42
            }
        "#;
    assert_eq!(run_vector(source), 42);
}

#[test]
fn struct_fields_nest_copy_and_run_on_the_machine() {
    let source = r#"
            #[repr(C)]
            struct Inner {
                a: u16,
                b: i16,
            }

            struct Point {
                x: u16,
                y: u16,
                inner: Inner,
                flags: Buf<u16, 2>,
                valid: bool,
            }

            fn main() {
                let mut p: Point = Point {
                    x: 3,
                    y: 4,
                    inner: Inner { a: 5, b: -6 },
                    flags: Buf::new([7, 8]),
                    valid: true,
                };
                p.x = p.x + 1u16;
                p.inner.a += 10u16;
                p.flags[1u16] = 9u16;
                p.y = p.flags[0u16];
                let copy: Point = p;
                let total = copy.x + copy.y + copy.inner.a + (copy.inner.b as u16)
                    + copy.flags[0u16] + copy.flags[1u16] + (copy.valid as u16);
                halt(total);
            }
        "#;
    // x = 3 + 1 = 4, y = flags[0] = 7, inner.a = 5 + 10 = 15, inner.b = 0xfffa,
    // flags = {7, 9}, valid = 1
    let expected = 4u16
        .wrapping_add(7)
        .wrapping_add(15)
        .wrapping_add((-6i16) as u16)
        .wrapping_add(7)
        .wrapping_add(9)
        .wrapping_add(1);
    assert_eq!(run_with_std_capped(source, 20_000), expected);
}

#[test]
fn struct_copy_is_an_independent_value() {
    let source = r#"
            struct Pair {
                a: u16,
                b: u16,
            }
            fn main() {
                let mut first: Pair = Pair { a: 1, b: 2 };
                let mut second: Pair = first;
                second.a = 40;
                first.b = 3;
                halt(first.a + first.b + second.a + second.b);
            }
        "#;
    // the copy took its own slot: 1 + 3 + 40 + 2 = 46
    assert_eq!(run_with_std_capped(source, 20_000), 46);
}

#[test]
fn struct_views_pass_by_pointer_and_index_fields() {
    let source = r#"
            struct Point { x: u16, y: u16 }

            fn sum_x(points: Array<Point>, count: u16) -> u16 {
                let mut total: u16 = 0;
                let mut i: u16 = 0;
                while i < count {
                    total = total + points[i].x;
                    i = i + 1u16;
                }
                total
            }

            fn bump(mut view: Array<Point>, dx: u16) {
                view[0u16].x = view[0u16].x + dx;
            }

            fn main() {
                let arr: Buf<Point, 3> = Buf::new([
                    Point { x: 10, y: 1 },
                    Point { x: 20, y: 2 },
                    Point { x: 30, y: 3 },
                ]);
                let view = arr.as_array();
                bump(view, 5u16);
                let total = sum_x(view, 3u16);
                let mut single: Point = Point { x: 7, y: 0 };
                bump(view_of(&mut single), 100u16);
                halt(total + single.x);
            }
        "#;
    // bump(view) makes x = 15, so sum_x = 65; bump(view_of(&mut single)) makes 107
    assert_eq!(run_with_std_capped(source, 40_000), 172);
}

#[test]
fn struct_view_index_scales_by_the_element_size() {
    // five words per element: a runtime index needs a real multiply, not a shift
    let source = r#"
            struct Wide { a: u16, b: u16, c: u16, d: u16, e: u16 }

            fn middle(view: Array<Wide>, i: u16) -> u16 {
                view[i].c
            }

            fn main() {
                let arr: Buf<Wide, 3> = Buf::new([
                    Wide { a: 1, b: 2, c: 3, d: 4, e: 5 },
                    Wide { a: 10, b: 20, c: 30, d: 40, e: 50 },
                    Wide { a: 100, b: 200, c: 300, d: 400, e: 500 },
                ]);
                let view = arr.as_array();
                let mut acc: u16 = 0;
                let mut i: u16 = 0;
                while i < 3u16 {
                    acc = acc + middle(view, i) + view[i].e;
                    i = i + 1u16;
                }
                halt(acc);
            }
        "#;
    // (3 + 5) + (30 + 50) + (300 + 500) = 888
    assert_eq!(run_with_std_capped(source, 40_000), 888);
}

#[test]
fn tuples_return_destructure_and_copy() {
    let source = r#"
            fn divmod_pair(a: u16, b: u16) -> (u16, u16) {
                (a / b, a % b)
            }

            fn stats(x: u16, y: u16) -> (u16, u16, u16) {
                let lo = if x < y { x } else { y };
                let hi = if x < y { y } else { x };
                (lo, hi, hi - lo)
            }

            fn main() {
                let t = divmod_pair(47u16, 5u16);
                let pair: (u16, u16) = t;
                let (q, r) = pair;
                let (lo, hi, span) = stats(9u16, 4u16);
                halt(q + r + lo + hi + span + t.0);
            }
        "#;
    // divmod_pair(47, 5) = (9, 2), stats(9, 4) = (4, 9, 5)
    assert_eq!(run_with_std_capped(source, 60_000), 9 + 2 + 4 + 9 + 5 + 9);
}

#[test]
fn struct_returns_write_through_the_hidden_pointer() {
    let source = r#"
            struct Point { x: u16, y: u16 }

            fn make(x: u16, y: u16) -> Point {
                Point { x: x, y: y }
            }

            fn shifted(p: Array<Point>, dx: u16) -> Point {
                Point { x: p[0u16].x + dx, y: p[0u16].y }
            }

            fn pass_through(p: Array<Point>) -> Point {
                shifted(p, 1u16)
            }

            fn main() {
                let mut p: Point = make(3u16, 4u16);
                let q: Point = pass_through(view_of(&mut p));
                let mut r: Point = q;
                r = make(10u16, 20u16);
                halt(p.x + p.y + q.x + q.y + r.x + r.y);
            }
        "#;
    // p = (3, 4), q = shifted(p, 1) = (4, 4), r = make(10, 20)
    assert_eq!(run_with_std_capped(source, 60_000), 3 + 4 + 4 + 4 + 10 + 20);
}

#[test]
fn fix32_struct_fields_preserve_bits_across_layout_views_and_returns() {
    let source = r#"
            struct Sample { head: u16, x: fix32, y: fix32, tail: u16 }
            struct Nested { tag: u16, value: Sample, end: u16 }
            fn check(ok: bool) { if !ok { halt(0xbad); } }
            fn make(x: fix32) -> Sample {
                Sample { head: 17, x, y: fix32::from_words(0xabcd, 0xffff), tail: 29 }
            }
            fn forwarded(x: fix32) -> Sample { make(x) }
            fn change(mut p: Array<Sample>, i: u16, value: fix32) {
                p[i].x = value;
            }
            fn main() {
                let f = fix32::from_words(0x1234, 1);
                let mut a = forwarded(f);
                check(a.head == 17 && a.tail == 29);
                check(a.x.lo_bits() == 0x1234 && a.x.hi_bits() == 1);
                check(a.y.lo_bits() == 0xabcd && a.y.hi_bits() == 0xffff);
                let mut n: Nested = Nested { tag: 5, value: a, end: 7 };
                n.value.x = n.value.x + fix32::from_int(2);
                check(n.value.x.lo_bits() == 0x1234 && n.value.x.hi_bits() == 3);
                check(n.tag == 5 && n.end == 7);
                let mut list: Buf<Sample, 2> = Buf::new([a, n.value]);
                change(list.as_array(), 1, fix32::from_words(0x8001, 0xfffe));
                check(list[0u16].x == f);
                check(list[1u16].x.lo_bits() == 0x8001 && list[1u16].x.hi_bits() == 0xfffe);
                check(list[1u16].head == 17 && list[1u16].tail == 29);
                a = list[1u16];
                check(a.x == list[1u16].x && a.y == list[0u16].y);
                // One-word head, then two-word x/y, then one-word tail.
                let raw = addr_of(&a);
                unsafe {
                    check(raw.read(0) == 17 && raw.read(1) == 0x8001 && raw.read(2) == 0xfffe);
                    check(raw.read(3) == 0xabcd && raw.read(4) == 0xffff && raw.read(5) == 29);
                };
                halt(0x600d);
            }
        "#;
    assert_eq!(run_with_std_capped(source, 100_000), 0x600d);
}

#[test]
fn static_fix32_fields_keep_exact_constant_constructor_bits() {
    let source = r#"
            struct State { head: u16, zero: fix32, negative: fix32, fraction: fix32, tail: u16 }
            static S: State = State { head: 19, zero: fix32::zero(),
                negative: fix32::from_int(-2), fraction: fix32::from_words(0x8123, 1), tail: 31 };
            fn check(ok: bool) { if !ok { halt(0xbad); } }
            fn main() {
                check(S.head == 19 && S.tail == 31);
                check(S.zero.lo_bits() == 0 && S.zero.hi_bits() == 0);
                check(S.negative.lo_bits() == 0 && S.negative.hi_bits() == 0xfffe);
                check(S.fraction.lo_bits() == 0x8123 && S.fraction.hi_bits() == 1);
                S.fraction = S.fraction + fix32::from_words(0, 0xffff);
                check(S.fraction.lo_bits() == 0x8123 && S.fraction.hi_bits() == 0);
                check(S.negative.hi_bits() == 0xfffe && S.tail == 31);
                halt(0x600d);
            }
        "#;
    assert_eq!(run_with_std_capped(source, 40_000), 0x600d);
}

#[test]
fn vector_lane_struct_returns_use_batched_stores_without_changing_field_bits() {
    for lanes in 2..=4 {
        let fields = ["x", "y", "z", "w"];
        let declarations = fields[..lanes]
            .iter()
            .map(|n| format!("{n}:fix32,"))
            .collect::<String>();
        let values = (0..lanes)
            .map(|i| {
                format!(
                    "fix32::from_words({}, {})",
                    100 + i * 257,
                    if i % 2 == 0 { 0 } else { 65535 }
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let initializers = fields[..lanes]
            .iter()
            .map(|n| format!("{n}:v.{n}(),"))
            .collect::<String>();
        let checks = fields[..lanes]
            .iter()
            .enumerate()
            .map(|(i, n)| {
                format!(
                    "check(s.{n}.lo_bits()=={} && s.{n}.hi_bits()=={});",
                    100 + i * 257,
                    if i % 2 == 0 { 0 } else { 65535 }
                )
            })
            .collect::<String>();
        let source = format!(
            r#"
                struct Result {{ head:u16,{declarations}tail:u16 }}
                fn check(ok:bool) {{ if !ok {{ halt(0xbad); }} }}
                fn make()->Result {{ let v=vec{lanes}::new({values});
                    Result {{ head:17,{initializers}tail:31 }} }}
                fn main() {{ let s=make();check(s.head==17&&s.tail==31);{checks}halt(0x600d); }}
            "#
        );
        let program = compile_with_std(&source);
        assert!(
            program.listing.contains(&format!("fstv.{lanes}")),
            "{}",
            program.listing
        );
        assert_eq!(execute_capped(program, 100_000).0, 0x600d);
    }
}

#[test]
fn buf_returns_and_struct_field_shorthand() {
    let source = r#"
            struct Pair { a: u16, b: u16 }

            fn table() -> Buf<u16, 3> {
                Buf::new([5, 6, 7])
            }

            fn pack(a: u16) -> Pair {
                Pair { a: a, b: a + 1u16 }
            }

            fn main() {
                let t: Buf<u16, 3> = table();
                let view = t.as_array();
                let p: Pair = pack(view[2u16]);
                halt(view[0u16] + p.a + p.b);
            }
        "#;
    // t = {5,6,7}; p = pack(7) = (7,8) => 5 + 7 + 8 = 20
    assert_eq!(run_with_std_capped(source, 40_000), 20);
}

#[test]
fn enums_are_words_and_carry_state() {
    let source = r#"
            #[derive(PartialEq)]
            enum Trace { Idle, Run, Halt }
            struct Job { state: Trace, ticks: u16 }

            fn next(s: Trace) -> Trace {
                if s == Trace::Idle { Trace::Run } else { Trace::Halt }
            }

            fn main() {
                let mut j: Job = Job { state: Trace::Idle, ticks: 0 };
                let first = j.state as u16;
                j.state = next(j.state);
                let second = j.state as u16;
                let third = next(j.state) as u16;
                let mut buf: Buf<Trace, 2> = Buf::new([Trace::Idle, Trace::Halt]);
                let mut v = buf.as_array();
                v[0u16] = j.state;
                let r0 = v[0u16] as u16;
                let r1 = v[1u16] as u16;
                halt(first * 1000u16 + second * 100u16 + third * 10u16 + r0 + r1);
            }
        "#;
    // Idle = 0, Run = 1, Halt = 2: first(0) second(1) third(2) r0(1) r1(2)
    assert_eq!(run_with_std_capped(source, 40_000), 100 + 20 + 1 + 2);
}

#[test]
fn owned_buffers_and_view_fields_use_their_element_addresses() {
    assert_source_in_both_modes(
        r#"
            struct Holder { data: Array<u16>, signed: Array<i16> }
            static GLOBAL: Buf<u16, 2> = Buf::new([10, 20]);
            fn main() {
                let mut b: Buf<u16, 2> = Buf::new([41, 42]);
                let mut s: Buf<i16, 2> = Buf::new([-3, -4]);
                b[0u16] += 1u16;
                s[1i16] = -5;
                let mut h: Holder = Holder { data: b.as_array(), signed: s.as_array() };
                if h.data[1u16] != 42u16 || h.signed[1u16] != -5i16 { halt(10); }
                h.data[0u16] = 50;
                h.signed[1u16] += 2i16;
                let i: u16 = 1;
                GLOBAL.as_array()[i] = b[i];
                halt(b[0u16] + GLOBAL[i] + (s[1u16] + 3i16) as u16);
            }
        "#,
        92,
    );
}

#[test]
fn buffer_struct_layout_dependencies_ignore_name_order() {
    assert_source_in_both_modes(
        r#"
            struct A { data: Buf<Z, 2> }
            struct Z { x: u16, y: u16 }
            fn main() {
                let a: A = A { data: Buf::new([Z { x: 1, y: 2 }, Z { x: 3, y: 4 }]) };
                halt(a.data[0u16].y + a.data[1u16].x);
            }
        "#,
        5,
    );
}

#[test]
fn enum_buffer_and_bool_tuple_survive_indirect_calls() {
    assert_source_in_both_modes(
        r#"
            #[derive(PartialEq)] enum State { Idle, Run, Done }
            fn next(s: State) -> State {
                if s == State::Idle { State::Run } else { State::Done }
            }
            fn flags(s: State) -> (bool, bool, u16) { (s == State::Run, s == State::Done, s as u16) }
            fn main() {
                let mut states: Buf<State, 3> = Buf::new([State::Idle, State::Run, State::Done]);
                let advance: fn(State) -> State = next;
                let mut total: u16 = 0;
                for i in 0u16..3 {
                    states[i] = advance(states[i]);
                    let (running, done, tag) = flags(states[i]);
                    if running { total += 10u16; }
                    if done { total += 100u16; }
                    total += tag;
                }
                halt(total);
            }
        "#,
        215,
    );
}

#[test]
fn destructive_operations_copy_the_source_when_both_remain_live() {
    // `a` is live across the shift and multiply, so the destructive forms
    // cannot reuse its register and must insert a MOV first.
    let source = r#"
            #[inline(never)] fn mix(x: u16) -> u16 {
                let a = x & 0x00ff;
                let b = a + 1;
                let s = a << 1;
                let m = a * 3;
                a + b + s + m
            }
            fn main() { halt(mix(100)); }
        "#;
    let program = compile(source, CompilerOptions::default());
    assert!(has_prefix(&program, "mov "), "{}", program.listing);
    assert!(has_prefix(&program, "shli"), "{}", program.listing);
    assert!(has_prefix(&program, "muli"), "{}", program.listing);
    // a = 100, b = 101, s = 200, m = 300
    assert_eq!(run(source), 701);
}

#[test]
fn if_value_arms_with_one_simple_instruction_use_a_conditional_move() {
    // `c3 - 1` is a single pure instruction, so the else arm stays
    // if-convertible; the countdown wraps 0 -> 2.
    let source = r#"
            #[inline(never)] fn countdown(c3: u16) -> u16 {
                if c3 == 0 { 2 } else { c3 - 1 }
            }
            fn main() {
                halt(countdown(0) * 100 + countdown(5));
            }
        "#;
    let program = compile(source, CompilerOptions::default());
    assert!(
        ["moveq", "movne", "movlt", "movge", "movgt", "movle"]
            .iter()
            .any(|m| has_prefix(&program, m)),
        "no conditional move\n{}",
        program.listing
    );
    assert!(has_prefix(&program, "subi"), "{}", program.listing);
    // countdown(0) = 2, countdown(5) = 4
    assert_eq!(run(source), 204);
}

#[test]
fn byte_sign_extend_and_leading_zero_count_use_their_instructions() {
    let source = r#"
            #[inline(never)] fn extend(x: u16) -> i16 { sextb(x) }
            #[inline(never)] fn bits(x: u16) -> u16 { clz(x) }
            fn main() {
                let a: i16 = extend(0x00ff); // -1
                let b: u16 = bits(0x8000);   // 0
                let c: u16 = bits(0x0001);   // 15
                halt((a as u16) + b + c);
            }
        "#;
    let program = compile(source, CompilerOptions::default());
    assert!(has_prefix(&program, "sextb"), "{}", program.listing);
    assert!(has_prefix(&program, "clz"), "{}", program.listing);
    // 0xffff + 0 + 15 wraps to 14
    assert_eq!(run(source), 14);
}
