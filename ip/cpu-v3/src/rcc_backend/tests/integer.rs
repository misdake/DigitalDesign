use super::*;

#[test]
fn typed_pointer_views_address_nested_records_without_storage() {
    let source = r#"
struct Entry { tag:u16, points:Buf<i16,3>, value:fix32 }
fn main(){
 let raw:Buf<u16,12>=Buf::new([0;12]);
 let mut rows=unsafe{raw.as_ptr().as_array::<Entry>()};
 rows[1u16].tag=17;
 rows[1u16].points[2u16]=-3;
 rows[1u16].value=fix32::from_words(0x1234,0x5678);
 if raw[9u16]!=65533 || raw[11u16]!=0x5678 {halt(1);}
 let words=unsafe{rows.as_ptr().as_array::<u16>()};
 halt(words[6u16]+words[10u16]);
}"#;
    assert_source_in_both_modes(source, 0x1245);
    let old =
        "fn main(){let p=Ptr::from_addr(0x100);let a=unsafe{p.as_u16_array()};halt(a[2u16]);}";
    let typed = old.replace("as_u16_array()", "as_array::<u16>()");
    let options = CompilerOptions::default();
    assert_eq!(
        compile(old, options.clone()).words,
        compile(&typed, options).words
    );
}

#[test]
fn named_constant_indices_use_the_literal_addressing_mode() {
    let source = "const FIELD:u16=5; #[inline(never)] fn literal(a:Array<u16>)->u16{a[5u16]} #[inline(never)] fn named(a:Array<u16>)->u16{a[FIELD]} #[inline(never)] fn expression(a:Array<u16>)->u16{a[FIELD+0u16]} fn main(){let a:Buf<u16,8>=Buf::new([1;8]);halt(literal(a.as_array())+named(a.as_array())+expression(a.as_array()));}";
    let program = compile(source, CompilerOptions::default());
    assert_eq!(execute_capped(program.clone(), 10_000).0, 3);
    let sizes = ["literal", "named", "expression"].map(|name| {
        let f = program
            .debug
            .functions
            .iter()
            .find(|f| f.name == name)
            .unwrap();
        f.addr.1 - f.addr.0 + 1
    });
    assert_eq!(sizes, [3, 3, 3], "{}", program.listing);
}

#[test]
fn integer_multiply_uses_the_hardware_mul() {
    let source = r#"
            fn main() {
                let a: u16 = 37;
                let b: u16 = 11;
                let mut acc: u16 = 0;
                let mut i: u16 = 0;
                while i < 8 {
                    acc = acc + a * b + i * 3;
                    i = i + 1;
                }
                halt(acc);
            }
        "#;
    let program = compile(source, CompilerOptions::default());
    assert!(
        !program.listing.contains("mul_16x"),
        "CpuV3 must not call the software multiply library:
{}",
        program.listing
    );
    // sum of (407 + 3i) for i in 0..8 = 3256 + 84
    assert_eq!(run(source), 3340);
}

#[test]
fn div_and_rem_run_on_the_machine() {
    let source = r#"
            fn main() {
                let a: u16 = 1000;
                let b: u16 = 7;
                let q = a / b;   // 142
                let r = a % b;   // 6
                let s: i16 = -1000;
                let t: i16 = 7;
                let u = s / t;   // -142: the quotient truncates toward zero
                let v = s % t;   // -6: the remainder follows the dividend
                let x: u16 = 0xabcd;
                let shifted = x / 16u16;  // literal power of two: a shift
                let masked = x % 16u16;   // literal power of two: a mask
                halt(q + r + (u + v + 300i16) as u16 + shifted + masked);
            }
        "#;
    // 142 + 6 + (-142 - 6 + 300) + 0x0abc + 0x000d = 3061
    assert_eq!(run_with_std(source), 3061);
}

#[test]
fn div_and_rem_define_a_zero_divisor() {
    let source = r#"
            fn main() {
                let z: u16 = 0;
                let zi: i16 = 0;
                let a: u16 = 1000;
                let s: i16 = -1000;
                if div_u16(a, z) == 0 && rem_u16(a, z) == a && div_i16(s, zi) == 0i16 && rem_i16(s, zi) == s {
                    halt(1);
                } else {
                    halt(0);
                }
            }
        "#;
    assert_eq!(run_with_std(source), 1);
}

#[test]
fn constant_divisor_magic_divide_runs_on_the_machine() {
    // A constant divisor lowers to the MUL16 magic path (the single-multiply
    // High form and the 17-bit Add form), so this compares the emitted
    // sequence against the host quotient across a range and at the edges.
    let source = r#"
            fn main() {
                let mut acc: u16 = 0;
                let mut x: u16 = 0;
                while x < 512u16 {
                    acc = acc + (x / 3u16) + (x % 3u16) + (x / 7u16) + (x % 7u16)
                        + (x / 10u16) + (x % 10u16) + (x / 1000u16) + (x % 1000u16);
                    x = x + 1;
                }
                acc = acc + (65535u16 / 3u16) + (65535u16 % 7u16) + (65534u16 / 65534u16)
                    + (65535u16 / 65534u16) + (40000u16 % 257u16) + (32768u16 / 9u16)
                    + (1u16 / 65535u16) + (65535u16 / 1u16);
                halt(acc);
            }
        "#;
    let mut expected: u16 = 0;
    for x in 0u16..512 {
        expected = expected
            .wrapping_add(x / 3)
            .wrapping_add(x % 3)
            .wrapping_add(x / 7)
            .wrapping_add(x % 7)
            .wrapping_add(x / 10)
            .wrapping_add(x % 10)
            .wrapping_add(x / 1000)
            .wrapping_add(x % 1000);
    }
    for term in [
        65535u16 / 3,
        65535u16 % 7,
        65534u16 / 65534,
        65535u16 / 65534,
        40000u16 % 257,
        32768u16 / 9,
        1u16 / 65535,
        // the source divides by 1u16; on the host that is the identity
        65535u16,
    ] {
        expected = expected.wrapping_add(term);
    }
    assert_eq!(run_with_std_capped(source, 500_000), expected);
}

#[test]
fn compound_assignments_run() {
    let source = r#"
            struct P { scale: u16 }

            fn main() {
                let mut a: u16 = 3;
                a *= 5u16;              // 15
                a /= 2u16;              // 7
                a %= 4u16;              // 3
                let mut buf: Buf<u16, 3> = Buf::new([1, 2, 3]);
                let mut view = buf.as_array();
                view[1u16] *= 4u16;
                let mut p: P = P { scale: 2 };
                p.scale *= 7u16;
                halt(a + buf.as_array()[1u16] + p.scale);
            }
        "#;
    // a = ((3 * 5) / 2) % 4 = 3, buf[1] = 8, p.scale = 14
    assert_eq!(run_with_std_capped(source, 20_000), 25);
}

#[test]
fn integer_division_and_remainder_compound_places_run() {
    let source = r#"
            struct Pair { unsigned: u16, signed: i16 }
            fn main() {
                let divisor: u16 = 7;
                let signed_divisor: i16 = 7;
                let mut u: u16 = 1000;
                let mut s: i16 = -1000;
                let mut p: Pair = Pair { unsigned: 1000, signed: -1000 };
                let mut ub: Buf<u16, 1> = Buf::new([1000]);
                let mut sb: Buf<i16, 1> = Buf::new([-1000]);
                u /= divisor; s /= signed_divisor;
                p.unsigned /= divisor; p.signed /= signed_divisor;
                ub[0u16] /= divisor; sb[0u16] /= signed_divisor;
                u %= 16; s %= 16;
                p.unsigned %= 16; p.signed %= 16;
                ub[0u16] %= 16; sb[0u16] %= 16;
                if u == 14 && s == -14 && p.unsigned == 14 && p.signed == -14
                    && ub[0u16] == 14 && sb[0u16] == -14 {
                    halt(0x600d);
                } else { halt(0xbad); }
            }
        "#;
    assert_eq!(run_with_std_capped(source, 60_000), 0x600d);
}

#[test]
fn aggregate_statics_land_in_the_data_section() {
    let source = r#"
            struct Sprite { x: u16, w: u16 }
            struct Level { count: u16, origin: Sprite, tags: Buf<u16, 2> }

            static TABLE: Buf<Sprite, 2> =
                Buf::new([Sprite { x: 3, w: 5 }, Sprite { x: 7, w: 9 }]);
            static LEVEL: Level =
                Level { count: 4, origin: Sprite { x: 1, w: 2 }, tags: Buf::new([8, 6]) };
            static PAIR: (u16, u16) = (10, 20);

            fn main() {
                let view = TABLE.as_array();
                view[1u16].w = 11u16;              // writable through a view
                let copy: Sprite = TABLE.as_array()[0u16];
                halt(copy.x + view[1u16].w + LEVEL.count + LEVEL.origin.x
                    + LEVEL.tags[1u16] + PAIR.0 + PAIR.1);
            }
        "#;
    // copy.x = 3, view[1].w = 11, count = 4, origin.x = 1, tags[1] = 6, PAIR = (10, 20)
    assert_eq!(
        run_with_std_capped(source, 20_000),
        3 + 11 + 4 + 1 + 6 + 10 + 20
    );
}

#[test]
fn aggregate_assignments_preserve_the_rhs_and_evaluate_it_first() {
    assert_source_in_both_modes(
        r#"
            struct P { x: u16, y: u16 }
            static INDEX: u16 = 0;
            fn swap(p: Array<P>) -> P { P { x: p[0u16].y, y: p[0u16].x } }
            fn make() -> P { unsafe { addr_of(&INDEX).write(0, 1) }; P { x: 7, y: 8 } }
            fn main() {
                let mut p: P = P { x: 1, y: 2 };
                p = P { x: p.y, y: p.x };
                if p.x != 2u16 || p.y != 1u16 { halt(10); }
                p = swap(view_of(&mut p));
                if p.x != 1u16 || p.y != 2u16 { halt(11); }
                let mut t: (u16, u16) = (3, 4);
                t = (t.1, t.0);
                if t.0 != 4u16 || t.1 != 3u16 { halt(12); }
                let mut b: Buf<u16, 2> = Buf::new([5, 6]);
                b = Buf::new([b[1u16], b[0u16]]);
                if b[0u16] != 6u16 || b[1u16] != 5u16 { halt(13); }
                let mut rows: Buf<P, 2> = Buf::new([P { x: 0, y: 0 }; 2]);
                rows[INDEX] = make();
                if rows[0u16].x != 0u16 || rows[1u16].x != 7u16 { halt(14); }
                halt(1);
            }
        "#,
        1,
    );
}

#[test]
fn recursive_sret_preserves_all_argument_registers_and_nested_destinations() {
    assert_source_in_both_modes(
        r#"
            struct Record { sum: u16, values: Buf<u16, 3> }
            struct Outer { item: Record, sentinel: u16 }
            fn make(a: u16, b: u16, c: u16, d: u16, e: u16) -> Record {
                Record { sum: a + b + c + d + e, values: Buf::new([a, b, c]) }
            }
            fn recur(n: u16, a: u16, b: u16, c: u16, d: u16) -> Record {
                if n == 0u16 { return make(a, b, c, d, 99u16); }
                let result: Record = recur(n - 1u16, a + 1u16, b + 2u16, c + 3u16, d + 4u16);
                return result;
            }
            fn main() {
                let mut outer: Outer = Outer { item: recur(3u16, 1u16, 2u16, 3u16, 4u16), sentinel: 777 };
                let saved: Record = outer.item;
                outer.item = recur(0u16, 9u16, 8u16, 7u16, 6u16);
                if saved.sum != 139u16 || saved.values[0u16] != 4u16
                    || saved.values[1u16] != 8u16 || saved.values[2u16] != 12u16 { halt(10); }
                if outer.item.sum != 129u16 || outer.item.values[0u16] != 9u16
                    || outer.item.values[1u16] != 8u16 || outer.item.values[2u16] != 7u16
                    || outer.sentinel != 777u16 { halt(11); }
                halt(1);
            }
        "#,
        1,
    );
}

#[test]
fn direct_and_indirect_calls_follow_the_cpu_v3_abi() {
    let source = r#"
            fn add(a: u16, b: u16) -> u16 { a + b }
            fn main() {
                let f: fn(u16, u16) -> u16 = add;
                halt(add(7, 8) + f(10, 20));
            }
        "#;
    assert_eq!(run(source), 45);
}

#[test]
fn nonzero_code_base_relocates_entry_and_function_addresses() {
    let source = r#"
            fn add(a: u16, b: u16) -> u16 { a + b }
            fn main() {
                let f: fn(u16, u16) -> u16 = add;
                halt(f(20, 22));
            }
        "#;
    let options = CompilerOptions {
        code_base: 0x0200,
        ..CompilerOptions::default()
    };
    let program = compile(source, options.clone());
    assert_eq!(program.code_base, 0x0200);
    assert!(program.listing.contains("main @ 0x0200"));
    assert_eq!(run_with_options(source, options).0, 42);
}

#[test]
fn local_arrays_and_bit_intrinsics_use_the_new_stack_and_operations() {
    let source = r#"
            fn main() {
                let mut words: Buf<u16, 4> = Buf::new([1, 2, 4, 8]);
                let mut view = words.as_array();
                view[2u16] = 0x800f;
                halt(view[0u16] + cnt1(view[2u16]) + log2(view[2u16]));
            }
        "#;
    assert_eq!(run(source), 21);
}

#[test]
fn segment_intrinsics_switch_data_and_code_segments() {
    let source = r#"
            fn main() {
                mtsr_dseg(1);
                let mut a = unsafe { Ptr::from_addr(0x0010).as_u16_array() };
                a[0u16] = 0x1234;
                jseg(2, 0x0020);
            }
        "#;
    let program = compile(source, CompilerOptions::default());
    let mut machine = cpu_v3::CpuV3Sim::default();
    machine
        .load_program(program.code_base, &program.words)
        .unwrap();
    let mut app = cpu_v3::load_immediate16(0, 0x77).to_vec();
    app.push(cpu_v3::halt());
    machine.load_segment(2, 0x0020, &app).unwrap();
    assert!(matches!(
        machine.run(1_000),
        Ok(cpu_v3::RunOutcome::Halted { signal: 0x77, .. })
    ));
    assert_eq!(machine.code_segment(), 2);
    assert_eq!(machine.data_segment(), 1);
    assert_eq!(
        machine.physical_memory(cpu_v3::PhysicalWordAddress::new(0x0001_0010)),
        0x1234
    );
}

#[test]
fn cache_handoff_is_one_terminal_ir_operation_and_two_adjacent_words() {
    let source = r#"
            fn main() {
                dcache_invalidate_all();
                let cseg: u16 = 2;
                let target: u16 = 0x0020;
                icache_invalidate_delayed_and_jump(cseg, target);
            }
        "#;
    let program = compile(source, CompilerOptions::default());
    assert_eq!(program.words.last(), Some(&cpu_v3::halt()));
    let tail = &program.words[program.words.len() - 3..program.words.len() - 1];
    assert_eq!(tail[0] & 0xfff0, 0x7800);
    assert_eq!(tail[1] & 0xff00, 0x6f00);
    assert_eq!(tail[0] & 0x000f, (tail[1] >> 4) & 0x000f);
    assert!(program.words[..program.words.len() - 3]
        .iter()
        .any(|word| word & 0xfff0 == 0x7810));
}

#[test]
fn line_copy_intrinsic_is_retained_and_maps_two_pointers() {
    let program = compile(
        r#"fn main() {
            unsafe { dcache_line_copy(Ptr::from_addr(0x120), Ptr::from_addr(0x4420)); };
            dcache_wait(); halt(0);
        }"#,
        CompilerOptions::default(),
    );
    assert!(program.words.iter().any(|word| word & 0xff00 == 0x6700));
    let mut machine = cpu_v3::CpuV3Sim::default();
    machine
        .load_program(program.code_base, &program.words)
        .unwrap();
    let words: [u16; 16] = std::array::from_fn(|i| 0xbe00 + i as u16);
    machine.load_program(0x120, &words).unwrap();
    assert!(matches!(
        machine.run(1000).unwrap(),
        cpu_v3::RunOutcome::Halted { signal: 0, .. }
    ));
    for (i, word) in words.into_iter().enumerate() {
        assert_eq!(
            machine.physical_memory(cpu_v3::PhysicalWordAddress::new(0x4420 + i as u32)),
            word
        );
    }
}

#[test]
fn removed_resident_copy_intrinsic_is_rejected() {
    let error = parse_source_with(
            "fn main(){unsafe{dcache_resident_copy(Ptr::from_addr(0x120),Ptr::from_addr(0x240));}halt(0);}",
            CompilerOptions::default().data_base,
        ).err().expect("removed intrinsic must not have a compatibility alias");
    assert!(
        error.to_string().contains("dcache_resident_copy"),
        "{error}"
    );
}

#[test]
fn cache_line_intrinsics_lower_to_dual_address_commands() {
    let source = r#"
            fn main() {
                let line = Ptr::from_addr(0x0100);
                unsafe {
                    dcache_line_copy(line, Ptr::from_addr(0x4100));
                    dcache_clean_line(line);
                };
                dcache_wait();
                halt(0);
            }
        "#;
    let program = compile(source, CompilerOptions::default());
    assert!(program.words.iter().any(|word| word & 0xff00 == 0x6700));
    assert!(program.words.iter().any(|word| word & 0xfff0 == 0x6e60));
    assert!(program.words.contains(&cpu_v3::wait_data_cache()));

    let mut machine = cpu_v3::CpuV3Sim::default();
    machine
        .load_program(program.code_base, &program.words)
        .unwrap();
    let line: [u16; 16] = std::array::from_fn(|i| 0x1000 + i as u16);
    machine.load_program(0x0100, &line).unwrap();
    let outcome = machine.run(10_000);
    assert!(
        matches!(outcome, Ok(cpu_v3::RunOutcome::Halted { signal: 0, .. })),
        "{outcome:?}"
    );
    for (index, expected) in line.into_iter().enumerate() {
        assert_eq!(
            machine.physical_memory(cpu_v3::PhysicalWordAddress::new(
                (1 << 14) | 0x0100 | index as u32
            )),
            expected
        );
    }
}

#[test]
fn non_overlapping_static_data_initializes_unified_memory() {
    let source = "static VALUE: u16 = 77; fn main() { halt(VALUE); }";
    let options = CompilerOptions {
        data_base: 0x4000,
        ..CompilerOptions::default()
    };
    let (signal, machine) = run_with_options(source, options);
    assert_eq!(signal, 77);
    assert_eq!(machine.memory(0x4000), 77);
}

#[test]
fn unified_code_and_static_data_overlap_is_rejected() {
    let source = "static VALUE: u16 = 7; fn main() { halt(VALUE); }";
    let options = CompilerOptions {
        data_base: 0,
        ..CompilerOptions::default()
    };
    let result = std::panic::catch_unwind(|| compile(source, options));
    let error = result.expect_err("overlapping CpuV3 code and static data must fail");
    let message = error
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| error.downcast_ref::<&str>().copied())
        .unwrap_or("");
    assert!(message.contains("CpuV3 unified-memory image"), "{message}");
}

#[test]
fn separate_segments_allow_code_and_static_data_to_use_the_same_offsets() {
    let source = "static VALUE: u16 = 7; fn main() { halt(VALUE); }";
    let options = CompilerOptions::for_separate_code_and_data_segments(0);
    assert_eq!(options.data_base, 0);
    assert_eq!(options.heap_begin, 0x2000);
    assert_eq!(options.heap_size, 0x7fff);
    assert_eq!(options.stack_init, 0xc000);
    assert!(options.separate_code_data_segments);

    let program = compile(source, options);
    assert_eq!(program.code_base, 0);
    assert!(!program.words.is_empty());
}

#[test]
fn segmented_application_uses_the_full_data_window_layout() {
    let source = r#"
            static MARK: u16 = 7;
            fn main() {
                let local: Buf<u16, 2> = Buf::new([3, 4]);
                let v = vec_new();
                vec_push(v, 11);
                vec_push(v, 22);
                if MARK != 7 || local[0u16] + local[1u16] != 7 { halt(10); }
                if vec_get(v, 0) != 11 || vec_get(v, 1) != 22 { halt(11); }
                vec_free(v);
                halt(1);
            }
        "#;
    let options = CompilerOptions::for_separate_code_and_data_segments(0);
    let program = compile_with_std_options(source, &options);

    let mut boot = Vec::new();
    boot.extend(cpu_v3::load_immediate16(1, 4));
    boot.extend(cpu_v3::load_immediate16(2, 3));
    boot.extend(cpu_v3::load_immediate16(3, 0));
    boot.extend(cpu_v3::load_immediate16(13, 0));
    boot.extend([cpu_v3::write_data_segment(1), cpu_v3::jump_segment(2, 3)]);

    let mut machine = cpu_v3::CpuV3Sim::default();
    machine.load_program(0, &boot).unwrap();
    machine.load_segment(3, 0, &program.words).unwrap();
    assert!(matches!(
        machine.run(100_000),
        Ok(cpu_v3::RunOutcome::Halted { signal: 1, .. })
    ));
    assert_eq!(machine.code_segment(), 3);
    assert_eq!(machine.data_segment(), 4);
    assert!((0xbf00..=0xbfff).contains(&machine.register(13).unwrap()));
    assert_eq!(
        machine.physical_memory(cpu_v3::PhysicalWordAddress::new(0x0004_2000)),
        0xffff
    );
}

#[test]
fn zero_stack_pointer_denotes_the_top_of_the_segment() {
    let source = r#"
            fn main() {
                let words: Buf<u16, 2> = Buf::new([0x1234, 0x4321]);
                let view = words.as_array();
                halt(view[0u16] + view[1u16]);
            }
        "#;
    assert_eq!(run(source), 0x5555);
}

#[test]
fn dynamic_shifts_select_register_count_encodings() {
    // Parameters keep the operands unknown so the optimizer cannot fold
    // the operations into a single constant.
    let source = r#"
            #[inline(never)] fn sh(a: u16, n: u16) -> u16 { a << n }
            #[inline(never)] fn lsr(a: u16, n: u16) -> u16 { a >> n }
            #[inline(never)] fn asr(a: i16, n: u16) -> i16 { a >> n }
            fn main() {
                let a: u16 = 3;
                let n: u16 = 1;
                let s: i16 = -8;
                halt(sh(a, n) + lsr(a, n) + (asr(s, n) as u16));
            }
        "#;
    let program = compile(source, CompilerOptions::default());
    assert!(has_prefix(&program, "shl "), "{}", program.listing);
    assert!(has_prefix(&program, "shr "), "{}", program.listing);
    assert!(has_prefix(&program, "asr "), "{}", program.listing);
    assert!(!has_prefix(&program, "shli"), "{}", program.listing);
    // 6 + 1 + 0xfffc wraps to 3
    assert_eq!(run(source), 3);
}

#[test]
fn constant_shifts_select_immediate_encodings() {
    let source = r#"
            #[inline(never)] fn sh(a: u16) -> u16 { a << 2 }
            #[inline(never)] fn shr(a: u16) -> u16 { a >> 1 }
            #[inline(never)] fn asr(a: i16) -> i16 { a >> 1 }
            fn main() {
                let a: u16 = 3;
                let s: i16 = -8;
                halt(sh(a) + shr(a) + (asr(s) as u16));
            }
        "#;
    let program = compile(source, CompilerOptions::default());
    assert!(has_prefix(&program, "shli"), "{}", program.listing);
    assert!(has_prefix(&program, "shri"), "{}", program.listing);
    assert!(has_prefix(&program, "asri"), "{}", program.listing);
    // 12 + 1 + 0xfffc wraps to 9
    assert_eq!(run(source), 9);
}

#[test]
fn immediate_arithmetic_families_select_short_and_wide_forms() {
    let source = r#"
            #[inline(never)] fn calc(x: u16) -> u16 {
                let a = x + 5;
                let b = a - 3;
                let c = b & 0x00ff; // does not fit u4: wide PFX12 form
                let d = c | 6;
                d ^ 7
            }
            fn main() { halt(calc(100)); }
        "#;
    let program = compile(source, CompilerOptions::default());
    for mnemonic in ["addi", "subi", "andi", "ori", "xori"] {
        assert!(
            has_prefix(&program, mnemonic),
            "missing {mnemonic}\n{}",
            program.listing
        );
    }
    assert!(
        disasm(&program)
            .iter()
            .any(|line| line.wide && line.text.starts_with("andi")),
        "andi 0x00ff must use the wide form\n{}",
        program.listing
    );
    // 100 + 5 - 3 = 102; & 0xff = 102; | 6 = 102; ^ 7 = 97
    assert_eq!(run(source), 97);
}

#[test]
fn immediate_selection_runs_with_optimizations_disabled() {
    // Legalization and immediate selection must not depend on the
    // optimizer; only the CFG-changing diamond conversion is opt-gated.
    let source = r#"
            fn calc(x: u16) -> u16 { x + 5 }
            fn main() { halt(calc(1)); }
        "#;
    let options = CompilerOptions {
        opt: rcc::Opts::disabled(),
        ..CompilerOptions::default()
    };
    let program = compile(source, options.clone());
    assert!(has_prefix(&program, "addi"), "{}", program.listing);
    assert_eq!(run_with_options(source, options).0, 6);
}

#[test]
fn multiply_lowers_to_hardware_windows_and_muli() {
    let source = r#"
            #[inline(never)] fn products(a: u16, b: u16) -> u16 {
                let lo = a * b;
                let w8 = mul8(a, b);
                let w16 = mul16(a, b);
                let m = a * 9;
                lo ^ w8 ^ w16 ^ m
            }
            fn main() { halt(products(0x00ff, 0x00ff)); }
        "#;
    let program = compile(source, CompilerOptions::default());
    for mnemonic in ["mul0", "mul8", "mul16", "muli"] {
        assert!(
            has_prefix(&program, mnemonic),
            "missing {mnemonic}\n{}",
            program.listing
        );
    }
    // 0xfe01 ^ 0x00fe ^ 0x0000 ^ 0x08f7 = 0xf608
    assert_eq!(run(source), 0xf608);
}
