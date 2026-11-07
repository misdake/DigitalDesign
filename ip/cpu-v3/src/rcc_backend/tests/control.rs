use super::*;

#[test]
fn conditional_integer_values_survive_constant_propagation() {
    let conditional = r#"
            fn pick(x:u16)->u16 {let v:u16=if x==0{2}else{1};v+1}
            fn main(){halt(pick(0)*10+pick(5));}
        "#;
    assert_eq!(run(conditional), 32);
    let source = r#"
            fn move_it(mut state:Array<u16>,command:u16){
                let mut x=state[0u16];let mut z=state[1u16];let mut orient=state[2u16];
                if orient==0 {
                    if command==0{x+=1;orient=1;}
                    if command==1{x-=2;orient=1;}
                    if command==2{z+=1;orient=2;}
                    if command==3{z-=2;orient=2;}
                }
                state[0u16]=x;state[1u16]=z;state[2u16]=orient;
            }
            fn main(){let mut s:Buf<u16,3>=Buf::new([0,1,0]);move_it(s.as_array(),2);halt(s[1u16]);}
        "#;
    assert_eq!(run(source), 2);
}

#[test]
fn conditional_array_selection_preserves_both_integer_arms() {
    let source = r#"
            static A:Buf<u16,32>=Buf::new([0;32]);static B:Buf<u16,16>=Buf::new([0;16]);
            fn state(which:bool)->u16 {
                let n:u16=if which{32}else{16};let index:u16=if which{3}else{4};
                let mut data=if which{A.as_array()}else{B.as_array()};
                data[0u16]=n;data[1u16]=index;n+index
            }
            fn main(){halt(state(true)*100+state(false));}
        "#;
    assert_eq!(run(source), 3520);
}

#[test]
fn conditional_integer_selection_preserves_fixed_point_comparison() {
    let source = r#"
            fn select(x:fix32)->u16 {let v=if x<fix32::from_int(1){2}else{3};v+1}
            fn main(){halt(select(fix32::zero())*10+select(fix32::from_int(2)));}
        "#;
    assert_eq!(run(source), 34);
}

#[test]
fn stored_bool_materializes_negates_casts_and_branches() {
    // A comparison bound to a variable becomes a 0/1 value (the ISA's
    // Boolean-producing form); it can be negated, cast, passed, returned and
    // used as a condition again, and `&&` builds a compound condition.
    let source = r#"
            fn is_small(x: u16) -> bool {
                x < 100u16
            }
            fn main() {
                let mut acc: u16 = 0;
                let mut x: u16 = 0;
                while x < 200u16 {
                    let small = is_small(x);
                    let odd = (x & 1u16) != 0u16;
                    let flag = small && odd;
                    if flag {
                        acc = acc + 1u16;
                    }
                    if !small {
                        acc = acc + 2u16;
                    }
                    acc = acc + (small as u16);
                    x = x + 1;
                }
                halt(acc);
            }
        "#;
    let mut expected: u16 = 0;
    for x in 0u16..200 {
        let small = x < 100;
        let odd = (x & 1) != 0;
        if small && odd {
            expected = expected.wrapping_add(1);
        }
        if !small {
            expected = expected.wrapping_add(2);
        }
        expected = expected.wrapping_add(u16::from(small));
    }
    assert_eq!(run_with_std_capped(source, 200_000), expected);
}

#[test]
fn bool_parameter_return_and_loop_condition() {
    let source = r#"
            fn keep_going(done: bool) -> bool {
                !done
            }
            fn main() {
                let mut i: u16 = 0;
                let mut go: bool = true;
                while go {
                    i = i + 1;
                    go = keep_going(i >= 5u16);
                }
                let pick = if 1u16 < 2u16 { 7u16 } else { 9u16 };
                halt(i + pick);
            }
        "#;
    // the loop stops the iteration i reaches 5, and the if-expression picks 7
    assert_eq!(run_with_std(source), 12);
}

#[test]
fn labeled_break_and_continue_leave_the_right_loop() {
    let source = r#"
            fn main() {
                let mut hits: u16 = 0;
                let mut rows: u16 = 0;
                'outer: for i in 0u16..4 {
                    let mut j: u16 = 0;
                    while j < 4u16 {
                        j = j + 1u16;
                        if i == 2u16 && j == 2u16 {
                            break 'outer;
                        }
                        if j == 1u16 {
                            continue;
                        }
                        hits = hits + 1u16;
                    }
                    rows = rows + 1u16;
                }
                let mut acc: u16 = 0;
                'again: for i in 0u16..4 {
                    let mut j: u16 = 0;
                    while j < 4u16 {
                        j = j + 1u16;
                        if j == 2u16 {
                            continue 'again;
                        }
                        acc = acc + 1u16;
                    }
                    acc = acc + 10u16;
                }
                halt(acc * 1000u16 + rows * 100u16 + hits);
            }
        "#;
    // rows = 2 (i = 0, 1), hits = 6 (3 per row), acc = 4: `continue 'again`
    // leaves the inner loop before it can finish, so the +10 never runs
    assert_eq!(run_with_std_capped(source, 60_000), 4 * 1000 + 2 * 100 + 6);
}

#[test]
fn match_lowers_to_a_branch_chain() {
    let source = r#"
            #[derive(PartialEq)]
            enum State { Idle, Run, Done }

            fn step(s: State) -> State {
                let mut next: State = State::Idle;
                match s {
                    State::Idle => { next = State::Run; }
                    State::Run => { next = State::Done; }
                    State::Done => { next = State::Idle; }
                }
                next
            }

            fn main() {
                let mut s: State = State::Idle;
                let mut acc: u16 = 0;
                let mut i: u16 = 0;
                while i < 6u16 {
                    i = i + 1u16;
                    s = step(s);
                    acc = acc + (s as u16);
                    match i {
                        3u16 => { acc = acc + 100u16; }
                        6u16 => { break; }
                        _ => { acc = acc + 1u16; }
                    }
                }
                halt(acc);
            }
        "#;
    // states cycle Run(1) Done(2) Idle(0) Run(1) Done(2) Idle(0):
    // acc = 1+2+0+1+2+0 = 6, plus a +1 on i=1,2,4,5 and +100 on i=3
    assert_eq!(run_with_std_capped(source, 60_000), 6 + 4 + 100);
}

#[test]
fn stored_boolean_operators_short_circuit_side_effects() {
    assert_source_in_both_modes(
        r#"
            static COUNT: u16 = 0;
            fn side(value: bool) -> bool {
                unsafe { addr_of(&COUNT).write(0, COUNT + 1u16) };
                value
            }
            fn choose(value: bool) -> bool { value || side(true) }
            fn main() {
                let a = false && side(true);
                let b = true || side(false);
                let c = side(true) && side(false);
                let d = side(false) || side(true);
                let e = !(false && side(true)) && choose(true);
                halt(COUNT * 100u16 + (a as u16) + (b as u16) * 2u16
                    + (c as u16) * 4u16 + (d as u16) * 8u16 + (e as u16) * 16u16);
            }
        "#,
        426,
    );
}

#[test]
fn division_boundaries_match_host_quotients_and_remainders() {
    let mut source = String::from(
        r#"
            fn unsigned(a: u16, b: u16, q: u16, r: u16) -> bool {
                let mut cq = a; cq /= b;
                let mut cr = a; cr %= b;
                a / b == q && a % b == r && cq == q && cr == r
            }
            fn signed(a: i16, b: i16, q: i16, r: i16) -> bool {
                let mut cq = a; cq /= b;
                let mut cr = a; cr %= b;
                a / b == q && a % b == r && cq == q && cr == r
            }
            fn main() {
        "#,
    );
    for (a, b) in [
        (0u16, 0u16),
        (1, 0),
        (65535, 0),
        (65535, 1),
        (65535, 2),
        (65535, 32768),
        (65535, 65535),
        (32768, 65535),
        (32768, 32767),
        (12345, 257),
        (40000, 3),
    ] {
        let q = a.checked_div(b).unwrap_or(0);
        let r = a.checked_rem(b).unwrap_or(a);
        source.push_str(&format!(
            "if !unsigned({a}u16, {b}u16, {q}u16, {r}u16) {{ halt(10); }}\n"
        ));
    }
    for (a, b) in [
        (i16::MIN, -1i16),
        (i16::MIN, 1),
        (i16::MIN, i16::MIN),
        (i16::MIN, 0),
        (i16::MAX, -1),
        (i16::MAX, i16::MIN),
        (-1000, 7),
        (1000, -7),
        (-1000, -7),
        (-1, 2),
        (1, -2),
        (0, -1),
    ] {
        let q = if b == 0 { 0 } else { a.wrapping_div(b) };
        let r = if b == 0 { a } else { a.wrapping_rem(b) };
        source.push_str(&format!(
            "if !signed({a}i16, {b}i16, {q}i16, {r}i16) {{ halt(11); }}\n"
        ));
    }
    source.push_str("halt(1); }");
    assert_source_in_both_modes(&source, 1);
}

#[test]
fn match_evaluates_once_and_composes_with_labeled_loop_exits() {
    assert_source_in_both_modes(
        r#"
            static CALLS: u16 = 0;
            fn observe(x: u16) -> u16 { unsafe { addr_of(&CALLS).write(0, CALLS + 1u16) }; x }
            fn classify(x: i16) -> u16 {
                let mut result: u16 = 0;
                match x {
                    -32768i16 => { result = 3; }
                    -1i16 => { result = 5; }
                    _ => { result = 7; }
                }
                result
            }
            fn main() {
                let mut total: u16 = 0;
                'rows: for row in 0u16..4 {
                    for col in 0u16..3 {
                        match observe(row) {
                            1u16 => { continue 'rows; }
                            3u16 => { break 'rows; }
                            _ => { total += row * 10u16 + col; }
                        }
                    }
                }
                // row 0 contributes 3, row 2 contributes 63; 3+1+3+1 observations.
                halt(total + CALLS * 100u16 + classify(-32768i16) + classify(-1i16) + classify(0i16));
            }
        "#,
        881,
    );
}

#[test]
fn frontend_ir_runs_arithmetic_loop_and_signed_comparisons() {
    let source = r#"
            fn main() {
                let mut sum: u16 = 0;
                let mut i: u16 = 5;
                while i != 0 { sum = sum + i; i = i - 1; }
                let low: i16 = -32768;
                let high: i16 = 32767;
                if low < high { halt(sum); } else { halt(99); }
            }
        "#;
    assert_eq!(run(source), 15);
}

#[test]
fn unsigned_comparisons_handle_values_near_the_wrap_boundary() {
    let source = r#"
            fn main() {
                let high: u16 = 0xffff;
                let low: u16 = 15;
                if high > low && low < high && high >= 0xffff && low <= 15 {
                    halt(7);
                } else {
                    halt(99);
                }
            }
        "#;
    assert_eq!(run(source), 7);
}

#[test]
fn segment_switch_mirror_loop_copies_across_data_segments() {
    let source = r#"
            fn main() {
                let desc = unsafe { Ptr::from_addr(0x1000).as_u16_array() };
                let mut handoff = unsafe { Ptr::from_addr(0x0100).as_u16_array() };
                let hseg: u16 = 2;
                let mut i: u16 = 0;
                while i < 32 {
                    let w = desc[i];
                    mtsr_dseg(hseg);
                    handoff[i] = w;
                    mtsr_dseg(0);
                    i = i + 1;
                }
                halt(0);
            }
        "#;
    let program = compile(source, CompilerOptions::default());
    let mut machine = cpu_v3::CpuV3Sim::default();
    machine
        .load_program(program.code_base, &program.words)
        .unwrap();
    let source_words: [u16; 32] = std::array::from_fn(|i| 0x100 + i as u16);
    machine.load_program(0x1000, &source_words).unwrap();
    match machine.run(100_000) {
        Ok(cpu_v3::RunOutcome::Halted { .. }) => {}
        other => panic!("mirror loop failed: {other:?}"),
    }
    for (i, &w) in source_words.iter().enumerate() {
        assert_eq!(
            machine.physical_memory(cpu_v3::PhysicalWordAddress::new(0x0002_0100 + i as u32)),
            w,
            "word {i}"
        );
    }
    assert_eq!(machine.data_segment(), 0);
}

#[test]
fn wide_immediate_comparisons_use_pfx12() {
    let source = r#"
            fn wide(x: u16) { if x < 1000 { halt(1); } else { halt(0); } }
            fn main() { wide(500); }
        "#;
    let program = compile(source, CompilerOptions::default());
    assert!(
        disasm(&program)
            .iter()
            .any(|line| line.wide && line.text.starts_with("cmpui")),
        "{}",
        program.listing
    );
    assert_eq!(run(source), 1);
}

#[test]
fn constant_on_the_left_swaps_the_condition() {
    // 40 > x must become x < 40 (not x >= 40), and -8 <= s must become
    // s >= -8, so the immediate compare keeps the written meaning.
    let source = r#"
            fn above(x: u16) { if 40 > x { halt(1); } else { halt(0); } }
            fn at_least(s: i16) { if -8 <= s { halt(2); } else { halt(0); } }
            fn main() { above(30); at_least(-5); }
        "#;
    let program = compile(source, CompilerOptions::default());
    assert!(has_prefix(&program, "cmpui"), "{}", program.listing);
    assert!(has_prefix(&program, "cmpsi"), "{}", program.listing);
    // above(30): 40 > 30 is true
    assert_eq!(run(source), 1);
}

#[test]
fn constant_comparisons_select_immediate_encodings() {
    // Statement branches stay branches (no diamond conversion), so the
    // compare must select CMPSI/CMPUI rather than a register compare.
    let source = r#"
            fn ucmp(x: u16) { if x < 40 { halt(1); } else { halt(0); } }
            fn scmp(s: i16) { if s >= -8 { halt(2); } else { halt(0); } }
            fn main() { ucmp(30); scmp(-5); }
        "#;
    let program = compile(source, CompilerOptions::default());
    assert!(has_prefix(&program, "cmpui"), "{}", program.listing);
    assert!(has_prefix(&program, "cmpsi"), "{}", program.listing);
    assert_eq!(run(source), 1);
}

#[test]
fn if_value_diamonds_become_boolean_comparisons_and_conditional_moves() {
    let source = r#"
            fn classify(x: u16) -> u16 {
                let flag = if x < 40 { 1 } else { 0 };
                let m = if x < 40 { x } else { 0 };
                let n = if x > 40 { x } else { 7 };
                flag + m + n
            }
            fn main() { halt(classify(30)); }
        "#;
    let program = compile(source, CompilerOptions::default());
    assert!(has_prefix(&program, "sltui"), "{}", program.listing);
    assert!(
        ["moveq", "movne", "movlt", "movge", "movgt", "movle"]
            .iter()
            .any(|m| has_prefix(&program, m)),
        "no conditional move\n{}",
        program.listing
    );
    // flag = 1, m = 30, n = 7
    assert_eq!(run(source), 38);
}

#[test]
fn if_value_with_a_register_false_arm_keeps_the_conditional_move() {
    // Regression: the `Mov dst, false` must survive CSE copy propagation,
    // which previously aliased it with the redefined result and dropped the
    // conditional write.
    let source = r#"
            fn clamp(x: u16) -> u16 { if x == 0 { 2 } else { x } }
            fn main() { halt(clamp(0) * 100 + clamp(5)); }
        "#;
    let program = compile(source, CompilerOptions::default());
    assert!(
        ["moveq", "movne", "movlt", "movge", "movgt", "movle"]
            .iter()
            .any(|m| has_prefix(&program, m)),
        "no conditional move\n{}",
        program.listing
    );
    // clamp(0) = 2, clamp(5) = 5
    assert_eq!(run(source), 205);
}

#[test]
fn branch_relaxation_selects_short_and_wide_forms() {
    // near target: the conditional branch relaxes to the one-word form
    let near = compile(
        r#"
                fn pick(x: u16) { if x == 1 { halt(7); } else { halt(9); } }
                fn main() { pick(1); }
            "#,
        CompilerOptions::default(),
    );
    assert!(
        disasm(&near).iter().any(|line| {
            !line.wide
                && ["beq", "bne", "blt", "bge", "bgt", "ble"]
                    .iter()
                    .any(|c| line.text.starts_with(c))
        }),
        "expected a short branch\n{}",
        near.listing
    );

    // far target: a 150-instruction loop body makes the back edge exceed
    // the signed 8-bit range, so it must stay in the wide form
    let mut body = String::new();
    for _ in 0..150 {
        body.push_str("acc = acc + x;\n");
    }
    let far_source = format!(
        "fn loopy(x: u16) {{ let mut i: u16 = 0; let mut acc: u16 = 0; \
             while i < 10 {{ {body} i = i + 1; }} halt(acc); }}\nfn main() {{ loopy(1); }}"
    );
    let far = compile(&far_source, CompilerOptions::default());
    assert!(
        disasm(&far).iter().any(|line| {
            line.wide
                && (["beq", "bne", "blt", "bge", "bgt", "ble", "jrel"]
                    .iter()
                    .any(|c| line.text.starts_with(c)))
        }),
        "expected a wide branch/jump\n{}",
        far.listing
    );
    // 10 iterations * 150 additions of x = 1
    assert_eq!(run(&far_source), 1500);
}
