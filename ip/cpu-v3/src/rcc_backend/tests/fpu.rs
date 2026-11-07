use super::*;

/// Scalar `fix32` lowers end to end through the two-word v2 ISA:
/// construction, arithmetic, unary operations, and numeric conversion.
#[test]
fn scalar_fix32_arithmetic_unary_and_conversions_run_on_the_machine() {
    let source = r#"
            fn main() {
                let a = fix32::from_int(7);
                let b = fix32::from_int(-2);
                let sum = a + b;             // 5
                let diff = a - b;            // 9
                let prod = a * b;            // -14
                let neg = -b;                // 2
                let fl = fix32::from_words(0x8000, 0x0001); // 1.5
                let down = fl.trunc();       // 1.0
                let up = fl.ceil();          // 2.0
                halt((sum + diff + prod + neg + down + up).to_int() as u16);
            }
        "#;
    // 5 + 9 - 14 + 2 + 1 + 2 = 5
    assert_eq!(run(source), 5);
}

/// The raw-half bridge (`ILO2F`/`IHI2F`/`FLO2I`/`FHI2I`) moves both words
/// without numeric conversion.
#[test]
fn scalar_fix32_raw_half_bridge_round_trips_words() {
    let source = r#"
            fn main() {
                let v = fix32::from_words(0x1234u16, 0xabcdu16);
                halt(v.lo_bits() ^ v.hi_bits());
            }
        "#;
    assert_eq!(run(source), 0x1234u16 ^ 0xabcdu16);
}

/// A fix32 value live across a call has no callee-saved F register to live
/// in, so it spills through a frame slot (`AddrOfFpuSpill` + FST/FLD).
#[test]
fn scalar_fix32_values_spill_across_calls() {
    let source = r#"
            fn addfix(a: fix32, b: fix32) -> fix32 { a + b }
            fn main() {
                let a = fix32::from_int(5);
                let b = addfix(a, fix32::from_int(1)); // `a` lives across the call
                halt((a + b).to_int() as u16);
            }
        "#;
    assert_eq!(run(source), 11);
}

/// fix32 comparisons feed the shared pending test; a materialized bool uses
/// a scalar `CMP` plus one conditional move.
#[test]
fn scalar_fix32_comparisons_branch_and_materialize_bools() {
    let source = r#"
            fn main() {
                let a = fix32::from_int(2);
                let b = fix32::from_int(3);
                let lt = a < b;
                let ge = a >= b;
                let eq = a == a;
                let le = a <= b;
                if lt && !ge && eq && le && b != a {
                    halt(1);
                } else {
                    halt(0);
                }
            }
        "#;
    assert_eq!(run(source), 1);
}

/// fix32 variables, loops and if-expressions exercise phis, compound
/// assignment and range-free scalar allocation.
#[test]
fn scalar_fix32_variables_loops_and_if_expressions_run() {
    let source = r#"
            fn main() {
                let mut acc = fix32::zero();
                let mut i = fix32::zero();
                let one = fix32::from_int(1);
                let limit = fix32::from_int(5);
                while i < limit {
                    acc += i;
                    i += one;
                }
                let pick = if acc > limit { acc } else { limit };
                halt((acc + pick).to_int() as u16);
            }
        "#;
    // acc = 0+1+2+3+4 = 10, pick = acc (10), total = 20
    assert_eq!(run(source), 20);
}

/// Vector parameters and returns pack through the frozen ABI: arguments
/// fill F4.. compactly by lane count and the result comes back in F0...
#[test]
fn vector_arguments_and_returns_follow_the_fpu_abi() {
    let source = r#"
            fn add3(a: vec3, b: vec3) -> vec3 { a + b }
            fn scale4(v: vec4, s: fix32) -> vec4 { v * s }
            fn dot3(a: vec3, b: vec3) -> fix32 { fdot(a, b) }
            fn main() {
                let a = vec3::new(fix32::from_int(1), fix32::from_int(2), fix32::from_int(3));
                let b = vec3::new(fix32::from_int(4), fix32::from_int(5), fix32::from_int(6));
                let c = add3(a, b);                 // 5, 7, 9
                let d = scale4(vec4::new(c.x(), c.y(), c.z(), fix32::zero()), fix32::from_int(2));
                let e = vec3::new(d.x(), d.y(), d.z()); // 10, 14, 18
                halt(dot3(e, e).to_int() as u16);   // 100 + 196 + 324 = 620
            }
        "#;
    assert_eq!(run_vector(source), 620);
}

/// `fdot` narrows the wide accumulator once; `vec4::import`/`export`
/// move four consecutive Q16.16 values through FLDV4/FSTV4.
#[test]
fn fdot_and_vec4_memory_import_export_run() {
    let source = r#"
            fn main() {
                let mut buf: Buf<u16, 8> = Buf::new([0; 8]);
                let ptr = buf.as_ptr();
                let v = vec4::new(fix32::from_int(2), fix32::from_int(3),
                                  fix32::from_int(4), fix32::from_int(5));
                vec4::export(v, ptr);
                let back = vec4::import(ptr);
                let d = fdot(back, back);           // 4 + 9 + 16 + 25 = 54
                halt(d.to_int() as u16);
            }
        "#;
    assert_eq!(run_vector(source), 54);
}

/// A vector loop accumulator becomes a phi over a contiguous range; the
/// back-edge parallel move must place all lanes consistently.
#[test]
fn vector_loop_accumulator_runs() {
    let source = r#"
            fn main() {
                let one = vec3::new(fix32::from_int(1), fix32::from_int(2), fix32::from_int(3));
                let mut acc = vec3::zero();
                let mut i: u16 = 0;
                while i < 5 {
                    acc += one;
                    i += 1;
                }
                halt((acc.x() + acc.y() + acc.z()).to_int() as u16); // 5*(1+2+3)=30
            }
        "#;
    assert_eq!(run_vector(source), 30);
}

/// A vector live across a call has no callee-saved F register, so it spills
/// through a wide FPU spill slot (FSTV/FLDV) and reloads after the call.
#[test]
fn vector_values_spill_across_calls_with_their_full_width() {
    let source = r#"
            fn keep(a: vec4, b: vec4) -> vec4 { a + b }
            fn main() {
                let v = vec4::new(fix32::from_int(1), fix32::from_int(2),
                                  fix32::from_int(3), fix32::from_int(4));
                let w = keep(v, vec4::new(fix32::from_int(10), fix32::from_int(10),
                                          fix32::from_int(10), fix32::from_int(10)));
                halt((v.x() + v.y() + v.z() + v.w()
                      + w.x() + w.y() + w.z() + w.w()).to_int() as u16);
            }
        "#;
    // v = 10, w = 10 + 40 = 50
    assert_eq!(run_vector(source), 60);
}

/// Lane shuffles lower through cycle-safe parallel moves, so a destination
/// range that overlaps the sources in any way still produces the permuted
/// result.
#[test]
fn vector_lane_shuffle_handles_overlapping_parallel_moves() {
    let source = r#"
            fn main() {
                let v = vec3::new(fix32::from_int(1), fix32::from_int(2), fix32::from_int(3));
                let s = vec3::new(v.z(), v.x(), v.y()); // 3, 1, 2
                halt((s.x() * fix32::from_int(100) + s.y() * fix32::from_int(10) + s.z())
                     .to_int() as u16);
            }
        "#;
    assert_eq!(run_vector(source), 312);
}

/// Range pressure: ten `vec4` pair sums stay live at once (40 lanes, more
/// than the 35-register allocatable area), so the allocator must spill and
/// reload multi-lane values at full width without breaking the overlap rule.
#[test]
fn vector_range_pressure_spills_and_stays_correct() {
    let mut source = String::from("fn main() {\n");
    for i in 0..10 {
        let v = i + 1;
        source.push_str(&format!(
            "let a{i} = vec4::new(fix32::from_int({v}), fix32::from_int({v}), \
                 fix32::from_int({v}), fix32::from_int({v}));\n"
        ));
    }
    for i in 0..10 {
        source.push_str(&format!("let p{i} = a{i} + a{i};\n"));
    }
    source.push_str("let mut s = p0;\n");
    for i in 1..10 {
        source.push_str(&format!("s += p{i};\n"));
    }
    source.push_str("halt((s.x() + s.y() + s.z() + s.w()).to_int() as u16);\n}\n");
    let program = compile(&source, CompilerOptions::default());
    assert_no_partial_overlap(&program);
    // each lane of p_i is 2*(i+1); four lanes per vector; sum over i
    let expected: u16 = (1..=10u16).map(|v| 8 * v).sum();
    assert_eq!(execute(program).0, expected);
}

/// The scalar FPU lowering emits the exact two-word v2 encodings; this
/// hand-built IR function pins the whole image (allocation is deterministic
/// for a straight-line function).
#[test]
fn scalar_fix32_add_emits_exact_two_word_words() {
    use rcc::{FpuBinOp, FuncBuilder, RegClass};
    let (mut b, params) = FuncBuilder::new_typed("main", &[RegClass::Fpu, RegClass::Fpu], 1);
    let a = b.get(params[0]);
    let c = b.get(params[1]);
    let sum = b.fpu_bin(FpuBinOp::Add, a, c);
    b.ret(&[sum]);
    let function = b.finish();
    let functions = std::collections::HashMap::from([("main", function)]);
    let program = compile_ir(
        functions,
        &CompilerOptions::default(),
        "main",
        rcc::frontend::FrontendDebug::default(),
    )
    .unwrap();
    // Pin the allocator/codegen contract, not merely decoder round-tripping:
    // the two ABI inputs are moved to f29/f28, the destructive ADD reuses
    // f28 as its destination, and the resulting pair is exactly these two
    // architectural words.
    let mut found = Vec::new();
    let mut index = 0;
    while index + 1 < program.words.len() {
        if let cpu_v3::Instruction::FpuScalar {
            fa,
            fb,
            fd,
            subop: cpu_v3::FpuScalarSubop::Add,
            ..
        } = cpu_v3::decode_fpu_pair(program.words[index], program.words[index + 1])
        {
            found.push((index, fa, fb, fd));
        }
        index += 1;
    }
    assert_eq!(found, vec![(4, 29, 28, 28)], "{}", program.listing);
    assert_eq!(&program.words[4..6], &[0xd75c, 0x7000]);
}

/// C3 lowers the scalar special functions to the two-word SCALAR special
/// subops: `frcp`/`frsqrt` use subops `0x0C`/`0x0D`, and `fsin`/`fcos`/
/// `fsincos` all reuse `SINCOS` (`0x0E`) with `mode[1:0]` selecting the
/// result. The dual output allocates a contiguous `Fd`/`Fd+1` pair.
#[test]
fn scalar_special_functions_emit_the_exact_subop_and_mode() {
    use rcc::{FpuSpecialOp, FuncBuilder, RegClass};
    for (op, subop, mode, lanes) in [
        (FpuSpecialOp::Rcp, cpu_v3::FpuScalarSubop::Rcp, 0u8, 1u8),
        (FpuSpecialOp::Rsqrt, cpu_v3::FpuScalarSubop::Rsqrt, 0, 1),
        (FpuSpecialOp::Sin, cpu_v3::FpuScalarSubop::SinCos, 1, 1),
        (FpuSpecialOp::Cos, cpu_v3::FpuScalarSubop::SinCos, 2, 1),
        (FpuSpecialOp::SinCos, cpu_v3::FpuScalarSubop::SinCos, 0, 2),
    ] {
        let (mut b, params) = FuncBuilder::new_typed("main", &[RegClass::Fpu], 1);
        let a = b.get(params[0]);
        let r = b.fpu_special(op, a, lanes);
        b.ret(&[r]);
        let function = b.finish();
        let functions = std::collections::HashMap::from([("main", function)]);
        let program = compile_ir(
            functions,
            &CompilerOptions::default(),
            "main",
            rcc::frontend::FrontendDebug::default(),
        )
        .unwrap();
        let mut found = Vec::new();
        let mut index = 0;
        while index + 1 < program.words.len() {
            if let cpu_v3::Instruction::FpuScalar {
                fd,
                subop: found_subop,
                mode: found_mode,
                ..
            } = cpu_v3::decode_fpu_pair(program.words[index], program.words[index + 1])
            {
                if found_subop == subop {
                    found.push((index, fd, found_mode));
                }
            }
            index += 1;
        }
        assert_eq!(
            found.len(),
            1,
            "{op:?} must emit exactly one special subop\n{}",
            program.listing
        );
        let (index, fd, found_mode) = found[0];
        assert_eq!(found_mode, mode, "{op:?}");
        // Check the emitted word1 field packing too: word0 is
        // {D, Fa, Fb=0}, word1 is {Fd, subop<<4 | mode}.
        assert_eq!(
            program.words[index + 1],
            (u16::from(fd) << 10) | (subop as u16) << 4 | u16::from(mode),
            "{op:?} word1"
        );
        // The dual-output form writes fd and fd+1.
        assert!(fd + lanes <= 64, "{op:?} destination range");
    }
}

/// C3 special functions are bit-exact with the architecture reference
/// model. Each result is folded into the halt signal with its own raw
/// halves, so a single register mix-up cannot cancel out.
#[test]
fn compiled_special_functions_match_the_reference_model() {
    let source = r#"
            fn main() {
                let x = fix32::from_words(0x0000u16, 0x0003u16); // 3.0
                let r = frcp(x);
                let s = frsqrt(x);
                let sc = fsincos(x);
                let si = fsin(x);
                let co = fcos(x);
                halt(r.lo_bits() ^ (s.hi_bits() << 1) ^ (sc.x().hi_bits() << 2)
                     ^ (sc.y().hi_bits() << 3) ^ (si.lo_bits() << 4)
                     ^ (co.hi_bits() << 5));
            }
        "#;
    let x = 3 << 16;
    let (sin, cos) = cpu_v3::sincos_q16(x);
    let expected = (cpu_v3::rcp_q16(x) as u16)
        ^ (((cpu_v3::rsqrt_q16(x) as u32 >> 16) as u16) << 1)
        ^ (((sin as u32 >> 16) as u16) << 2)
        ^ (((cos as u32 >> 16) as u16) << 3)
        ^ ((sin as u16) << 4)
        ^ (((cos as u32 >> 16) as u16) << 5);
    assert_eq!(run(source), expected);
}

/// The vector lowering emits the exact two-word VECTOR encoding. A vec3
/// `VADD` packs `Fa`/`Fb` into word 0 and `Fd`/len/subop/mode into word 1;
/// this hand-built IR pins the allocator/codegen contract, including the
/// in-place destination reuse of the first source.
#[test]
fn vector_vadd_emits_exact_two_word_words() {
    use rcc::{FpuBinOp, FuncBuilder, RegClass};
    let (mut b, params) = FuncBuilder::new_typed("main", &[RegClass::Fpu, RegClass::Fpu], 1);
    b.set_vreg_lanes(params[0] as u32, 3);
    b.set_vreg_lanes(params[1] as u32, 3);
    let a = b.get(params[0]);
    let c = b.get(params[1]);
    let sum = b.fpu_vec_bin(FpuBinOp::Add, a, c, 3);
    b.ret(&[sum]);
    let function = b.finish();
    let functions = std::collections::HashMap::from([("main", function)]);
    let program = compile_ir(
        functions,
        &CompilerOptions::default(),
        "main",
        rcc::frontend::FrontendDebug::default(),
    )
    .unwrap();
    let mut found = Vec::new();
    let mut index = 0;
    while index + 1 < program.words.len() {
        if let cpu_v3::Instruction::FpuVector {
            fa,
            fb,
            fd,
            len,
            subop: cpu_v3::FpuVectorSubop::VAdd,
            ..
        } = cpu_v3::decode_fpu_pair(program.words[index], program.words[index + 1])
        {
            found.push((index, fa, fb, fd, len));
        }
        index += 1;
    }
    assert_eq!(
        found,
        vec![(12, 31, 28, 28, cpu_v3::FpuVectorLength::Vec3)],
        "{}",
        program.listing
    );
    // VADD.3 f28..f30, f31..f33, f28..f30 (in-place with Fb):
    // word0 = 0xC000 | (Fa=31 << 6) | Fb=28; word1 = Fd=28 << 10 | len=01 << 8.
    assert_eq!(&program.words[12..14], &[0xc7dc, 0x7100]);
}

/// `VMULS` pins the broadcast encoding: the scalar source shares word 0
/// with the vector base and the subop sits in word 1.
#[test]
fn vector_vmuls_emits_exact_two_word_words() {
    use rcc::{FuncBuilder, RegClass};
    let (mut b, params) = FuncBuilder::new_typed("main", &[RegClass::Fpu, RegClass::Fpu], 1);
    b.set_vreg_lanes(params[0] as u32, 3);
    let v = b.get(params[0]);
    let s = b.get(params[1]);
    let r = b.fpu_vec_muls(v, s, 3);
    b.ret(&[r]);
    let function = b.finish();
    let functions = std::collections::HashMap::from([("main", function)]);
    let program = compile_ir(
        functions,
        &CompilerOptions::default(),
        "main",
        rcc::frontend::FrontendDebug::default(),
    )
    .unwrap();
    let mut found = Vec::new();
    let mut index = 0;
    while index + 1 < program.words.len() {
        if let cpu_v3::Instruction::FpuVector {
            fa,
            fb,
            fd,
            len,
            subop: cpu_v3::FpuVectorSubop::VMulS,
            ..
        } = cpu_v3::decode_fpu_pair(program.words[index], program.words[index + 1])
        {
            found.push((index, fa, fb, fd, len));
        }
        index += 1;
    }
    assert_eq!(
        found,
        vec![(8, 29, 28, 29, cpu_v3::FpuVectorLength::Vec3)],
        "{}",
        program.listing
    );
    // VMULS.3 f29..f31 = f29..f31 * f28:
    // word0 = 0xC000 | (Fa=29 << 6) | Fb=28; word1 = Fd=29 << 10 | len=01 | VMULS(3) << 3.
    assert_eq!(&program.words[8..10], &[0xc75c, 0x7518]);
}

/// A vector live across a call spills through wide FLDV/FSTV traffic: the
/// `mode` field is lanes-1, so a vec3 spill uses mode 2 and never a scalar
/// FLD/FST or the wrong width.
#[test]
fn vector_spill_across_a_call_uses_full_width_fldv_fstv() {
    let source = r#"
            fn keep(a: vec3, b: vec3) -> vec3 { a + b }
            fn main() {
                let v = vec3::new(fix32::from_int(1), fix32::from_int(2), fix32::from_int(3));
                let w = keep(v, v);
                halt((v.x() + w.x()).to_int() as u16);
            }
        "#;
    let program = compile(source, CompilerOptions::default());
    assert_no_partial_overlap(&program);
    let mut vec_loads = 0;
    let mut vec_stores = 0;
    let mut index = 0;
    while index + 1 < program.words.len() {
        if let cpu_v3::Instruction::FpuAux { subop, mode, .. } =
            cpu_v3::decode_fpu_pair(program.words[index], program.words[index + 1])
        {
            if subop == cpu_v3::FpuAuxSubop::Fld as u8 && mode == 2 {
                vec_loads += 1;
            }
            if subop == cpu_v3::FpuAuxSubop::Fst as u8 && mode == 2 {
                vec_stores += 1;
            }
        }
        index += 1;
    }
    assert!(
        vec_loads > 0 && vec_stores > 0,
        "expected a vec3 spill through FLDV/FSTV mode 2\n{}",
        program.listing
    );
    // v.x() + w.x() = 1 + 2 = 3
    assert_eq!(execute(program).0, 3);
}

/// The compiled program used by the system-level emulator/RTL co-simulation:
/// vec4 construction, a vector call, VMULS, lane extraction and `fdot`
/// together produce a single deterministic halt signal on the emulator.
#[test]
fn compiled_vec4_abi_vmuls_and_fdot_program_runs() {
    let source = r#"
            fn add4(a: vec4, b: vec4) -> vec4 { a + b }
            fn main() {
                let a = vec4::new(fix32::from_int(1), fix32::from_int(2),
                                  fix32::from_int(3), fix32::from_int(4));
                let b = vec4::new(fix32::from_int(10), fix32::from_int(20),
                                  fix32::from_int(30), fix32::from_int(40));
                let c = add4(a, b);
                let d = c * fix32::from_int(2);
                let s = d.x() + d.y() + d.z() + d.w();
                let dot = fdot(c, d);
                halt((s + dot).to_int() as u16);
            }
        "#;
    assert_eq!(run_vector(source), 7480);
}

/// The backend no longer refuses FPU programs: an FPU-typed function
/// pointer plus a scalar body compiles.
#[test]
fn an_fpu_class_program_is_now_lowered_by_the_backend() {
    let source = r#"
            fn identity(v: fix32) -> fix32 { v }
            fn main() {
                let keep: fn(fix32) -> fix32 = identity;
                halt(keep(fix32::from_int(9)).to_int() as u16);
            }
        "#;
    assert_eq!(run(source), 9);
}

/// C0 freezes the Q16.16 contiguous-range ABI even though range-aware
/// allocation lands with C2.
#[test]
fn cpu_v3_fpu_convention_uses_the_frozen_q16_layout() {
    let fpu = CPU_V3_REGISTER_CONVENTION.fpu.unwrap();
    assert_eq!(fpu.return_registers, &[0, 1, 2, 3]);
    assert_eq!(fpu.argument_registers.first(), Some(&4));
    assert_eq!(fpu.argument_registers.last(), Some(&27));
    assert_eq!(fpu.allocatable_registers.first(), Some(&28));
    assert_eq!(fpu.allocatable_registers.last(), Some(&62));
    assert_eq!(fpu.scratch_register, 63);
}
