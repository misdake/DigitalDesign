use super::abi::REG_TMP;
use super::*;
use crate as cpu_v3;
use rcc::frontend::parse_source_with;

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

/// Every emitted hardware VECTOR instruction obeys the design's section
/// 3.2 partial-overlap rule: a destination range either shares a source
/// base exactly or is disjoint from it, and a `VMULS` scalar is never
/// inside the destination range.
fn assert_no_partial_overlap(program: &CpuV3Program) {
    let mut index = 0;
    while index + 1 < program.words.len() {
        if let cpu_v3::Instruction::FpuVector {
            fa,
            fb,
            fd,
            len,
            subop,
            ..
        } = cpu_v3::decode_fpu_pair(program.words[index], program.words[index + 1])
        {
            let lanes = usize::from(len.lanes());
            let (fa, fb, fd) = (usize::from(fa), usize::from(fb), usize::from(fd));
            let overlap = |a: usize, b: usize, blen: usize| a != b && a < b + blen && b < a + lanes;
            assert!(
                !overlap(fd, fa, lanes),
                "destination f{fd}.. partially overlaps Fa f{fa}.. in:\n{}",
                program.listing
            );
            if subop == cpu_v3::FpuVectorSubop::VMulS {
                assert!(
                    !(fb >= fd && fb < fd + lanes),
                    "VMULS scalar f{fb} is inside destination f{fd}.. in:\n{}",
                    program.listing
                );
            } else if !subop.ignores_second_source() && !subop.uses_stride_mode() {
                assert!(
                    !overlap(fd, fb, lanes),
                    "destination f{fd}.. partially overlaps Fb f{fb}.. in:\n{}",
                    program.listing
                );
            }
        }
        index += 1;
    }
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

#[test]
fn debug_info_covers_modules_lines_and_locals() {
    let source = r#"
            mod helper;
            fn main() {
                let x: u16 = double(21);
                halt(x);
            }
        "#;
    let module = "fn double(v: u16) -> u16 { v + v }";
    let program = rcc::frontend::compile_program_named(
        "main.rs",
        source,
        &CompilerOptions::default(),
        &mut |name| {
            assert_eq!(name, "helper");
            Ok(module.to_string())
        },
    )
    .unwrap();
    let program = super::compile(program, &CompilerOptions::default(), "main");
    let debug = &program.debug;
    // both files recorded
    assert!(debug.files.len() >= 2, "files: {:?}", debug.files);
    // the line map is non-empty, sorted, and in range
    assert!(!debug.lines.is_empty());
    assert!(debug.lines.windows(2).all(|w| w[0].0 <= w[1].0));
    let main_fn = debug
        .functions
        .iter()
        .find(|f| f.name == "main")
        .expect("main in debug info");
    assert!(main_fn.addr.1 > main_fn.addr.0);
    // `x` is a local in main with a concrete lowered location
    let x = main_fn
        .locals
        .iter()
        .find(|v| v.name == "x")
        .expect("local x in debug info");
    match x.loc {
        rcc::VarLoc::Frame(_) | rcc::VarLoc::Param(_) | rcc::VarLoc::Ssa => {}
        ref other => panic!("unexpected location for x: {other:?}"),
    }
    // render/parse round-trip
    let parsed = rcc::parse_debug(&debug.render()).expect("debug info round-trips");
    assert_eq!(parsed.files.len(), debug.files.len());
    assert_eq!(parsed.lines.len(), debug.lines.len());
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

fn compile(source: &str, options: CompilerOptions) -> CpuV3Program {
    let program = parse_source_with(source, options.data_base).unwrap();
    super::compile(program, &options, "main")
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

fn assert_source_in_both_modes(source: &str, expected: u16) {
    for opt in [rcc::Opts::default(), rcc::Opts::disabled()] {
        let options = CompilerOptions {
            opt,
            ..CompilerOptions::default()
        };
        let program =
            rcc::frontend::compile_program_named("<regression>", source, &options, &mut |name| {
                Err(format!("unknown module `{name}`"))
            })
            .unwrap();
        let compiled = super::compile(program, &options, "main");
        assert_eq!(execute_capped(compiled, 100_000).0, expected);
    }
}

/// Compile with the rcc standard library appended. `compile` above is
/// parse-only, so operators that lower to a library call (such as `/`) and
/// the library modules themselves need this entry point.
fn compile_with_std(source: &str) -> CpuV3Program {
    let options = CompilerOptions::default();
    compile_with_std_options(source, &options)
}

fn compile_with_std_options(source: &str, options: &CompilerOptions) -> CpuV3Program {
    let program = rcc::frontend::compile_program_named("<test>", source, options, &mut |name| {
        Err(format!("unknown module `{name}`"))
    })
    .unwrap();
    super::compile(program, options, "main")
}

fn execute(program: CpuV3Program) -> (u16, cpu_v3::CpuV3Sim) {
    execute_capped(program, 10_000)
}

fn execute_capped(program: CpuV3Program, max_cycles: usize) -> (u16, cpu_v3::CpuV3Sim) {
    let mut machine = cpu_v3::CpuV3Sim::default();
    machine
        .load_program(program.code_base, &program.words)
        .unwrap();
    if program.code_base != 0 {
        let mut bootstrap = cpu_v3::load_immediate16(REG_TMP, program.code_base).to_vec();
        bootstrap.push(cpu_v3::jump_register(REG_TMP));
        machine.load_program(0, &bootstrap).unwrap();
    }
    let signal = match machine.run(max_cycles).unwrap() {
        cpu_v3::RunOutcome::Halted { signal, .. } => signal,
        outcome => panic!("CpuV3 program did not halt: {outcome:?}"),
    };
    (signal, machine)
}

fn run(source: &str) -> u16 {
    run_with_options(source, CompilerOptions::default()).0
}

/// like `run`, but first asserts the section 3.2 partial-overlap rule over
/// every emitted VECTOR instruction (C2 lowering)
fn run_vector(source: &str) -> u16 {
    let program = compile(source, CompilerOptions::default());
    assert_no_partial_overlap(&program);
    execute(program).0
}

fn run_with_options(source: &str, options: CompilerOptions) -> (u16, cpu_v3::CpuV3Sim) {
    execute(compile(source, options))
}

fn run_with_std(source: &str) -> u16 {
    execute(compile_with_std(source)).0
}

/// like `run_with_std`, for a program that needs more than the default cap
fn run_with_std_capped(source: &str, max_cycles: usize) -> u16 {
    execute_capped(compile_with_std(source), max_cycles).0
}

fn run_with_std_options(
    source: &str,
    options: CompilerOptions,
    max_cycles: usize,
) -> (u16, cpu_v3::CpuV3Sim) {
    execute_capped(compile_with_std_options(source, &options), max_cycles)
}

#[test]
fn heap_realloc_grows_in_place_and_preserves_words() {
    let source = r#"
            fn main() {
                let p = malloc(4);
                unsafe {
                    p.write(0, 11);
                    p.write(1, 22);
                    p.write(2, 33);
                    p.write(3, 44);
                };
                let q = heap_realloc(p, 8);
                if q.addr() != p.addr() { halt(10); }
                if unsafe { q.read(0) } != 11 || unsafe { q.read(3) } != 44 { halt(11); }
                let r = heap_realloc(q, 2);
                if r.addr() != p.addr() || unsafe { r.read(1) } != 22 { halt(12); }
                free(r);
                halt(1);
            }
        "#;
    let options = CompilerOptions {
        heap_size: 24,
        ..CompilerOptions::default()
    };
    assert_eq!(run_with_std_options(source, options, 100_000).0, 1);
}

#[test]
fn heap_realloc_falls_back_and_best_fit_prefers_the_smaller_hole() {
    let source = r#"
            fn main() {
                let a = malloc(4);
                let b = malloc(10);
                let guard1 = malloc(4);
                let small = malloc(6);
                let guard2 = malloc(4);
                unsafe { a.write(0, 77) };
                let moved = heap_realloc(a, 8);
                if moved.addr() == a.addr() || unsafe { moved.read(0) } != 77 { halt(11); }
                free(b);
                free(small);
                let fit = malloc(5);
                if fit.addr() != small.addr() { halt(10); }
                free(fit);
                free(moved);
                free(guard1);
                free(guard2);
                halt(1);
            }
        "#;
    let options = CompilerOptions {
        heap_size: 64,
        ..CompilerOptions::default()
    };
    assert_eq!(run_with_std_options(source, options, 100_000).0, 1);
}

#[test]
fn vec_grows_with_the_default_heap_and_supports_reserve_and_shrink() {
    let source = r#"
            fn main() {
                let v = vec_new();
                let mut i: u16 = 0;
                while i < 8 {
                    vec_push(v, i + 10);
                    i += 1;
                }
                if vec_len(v) != 8 || vec_cap(v) < 8 { halt(10); }
                if vec_get(v, 0) != 10 || vec_get(v, 7) != 17 { halt(11); }
                while vec_len(v) > 3 { vec_pop(v); }
                vec_shrink_to_fit(v);
                if vec_cap(v) != 3 || vec_get(v, 2) != 12 { halt(12); }
                vec_clear(v);
                if !vec_is_empty(v) { halt(13); }
                vec_reserve(v, 4);
                if vec_cap(v) < 4 { halt(14); }
                vec_free(v);
                halt(1);
            }
        "#;
    assert_eq!(run_with_std_capped(source, 100_000), 1);
}

#[test]
fn zero_capacity_vec_can_grow_and_checked_pop_rejects_empty() {
    let grow = r#"
            fn main() {
                let v = vec_new();
                vec_push(v, 7);
                vec_push(v, 8);
                vec_push(v, 9);
                vec_push(v, 10);
                halt(vec_cap(v) * 100 + vec_get(v, 3));
            }
        "#;
    let options = CompilerOptions {
        heap_size: 20,
        vec_init_cap: 0,
        ..CompilerOptions::default()
    };
    assert_eq!(run_with_std_options(grow, options, 100_000).0, 410);

    let empty_pop = "fn main() { let v = vec_new(); vec_pop(v); halt(1); }";
    assert_eq!(run_with_std(empty_pop), 0xffe3);
}

#[test]
fn heap_coalesces_both_sides_and_rejects_double_free() {
    let coalesce = r#"
            fn main() {
                let a = malloc(4);
                let b = malloc(4);
                let c = malloc(4);
                free(a);
                free(c);
                free(b);
                let whole = malloc(18);
                if whole.addr() != a.addr() { halt(10); }
                free(whole);
                halt(1);
            }
        "#;
    let options = CompilerOptions {
        heap_size: 24,
        ..CompilerOptions::default()
    };
    assert_eq!(run_with_std_options(coalesce, options, 100_000).0, 1);

    let double_free = "fn main() { let p = malloc(1); free(p); free(p); halt(1); }";
    assert_eq!(run_with_std(double_free), 0xffe2);
}

#[test]
fn heap_at_zero_supports_exact_fit_and_reports_oom() {
    let source = r#"
            fn main() {
                let p = malloc(6);
                if p.addr() != 1 { halt(10); }
                unsafe { p.write(5, 77) };
                if unsafe { p.read(5) } != 77 { halt(11); }
                free(p);
                malloc(7);
                halt(1);
            }
        "#;
    let options = CompilerOptions {
        code_base: 0x100,
        heap_begin: 0,
        heap_size: 8,
        ..CompilerOptions::default()
    };
    assert_eq!(run_with_std_options(source, options, 100_000).0, 0xffe1);

    let maximum = r#"
            fn main() {
                let p = malloc(0x7ffd);
                if p.addr() != 1 { halt(10); }
                unsafe { p.write(32764i16, 77) };
                if unsafe { p.read(32764i16) } != 77 { halt(11); }
                halt(1);
            }
        "#;
    let maximum_options = CompilerOptions {
        code_base: 0x8000,
        data_base: 0x9000,
        heap_begin: 0,
        heap_size: 0x7fff,
        ..CompilerOptions::default()
    };
    assert_eq!(run_with_std_options(maximum, maximum_options, 100_000).0, 1);
}

#[test]
fn vec_checked_access_and_capacity_failures_are_stable() {
    let out_of_bounds = "fn main() { let v = vec_new(); vec_push(v, 7); vec_get(v, 1); halt(1); }";
    assert_eq!(run_with_std(out_of_bounds), 0xffe3);

    let overflow = "fn main() { let v = vec_new(); vec_reserve(v, 0xffff); halt(1); }";
    assert_eq!(run_with_std(overflow), 0xffe4);
}

fn disasm(program: &CpuV3Program) -> Vec<cpu_v3::DisasmLine> {
    cpu_v3::disassemble_words(&program.words, program.code_base)
}

fn mnemonics(program: &CpuV3Program) -> Vec<String> {
    disasm(program).into_iter().map(|line| line.text).collect()
}

fn has_prefix(program: &CpuV3Program, prefix: &str) -> bool {
    mnemonics(program).iter().any(|m| m.starts_with(prefix))
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
fn device_intrinsics_use_the_dedicated_device_path() {
    let source = r#"
            const ECHO_DEVICE: u16 = 1 + 1;
            const ECHO_CHANNEL: u16 = 1 + 2;
            fn main() {
                dev_send(ECHO_DEVICE, ECHO_CHANNEL, 0x1234);
                halt(dev_recv(ECHO_DEVICE, ECHO_CHANNEL));
            }
        "#;
    struct EchoDevice([u16; 16]);
    impl cpu_v3::Device for EchoDevice {
        fn read(&mut self, _memory: &mut [u16], channel: u8) -> u16 {
            self.0[usize::from(channel)]
        }
        fn write(&mut self, _memory: &mut [u16], channel: u8, value: u16) {
            self.0[usize::from(channel)] = value;
        }
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }
    let program = compile(source, CompilerOptions::default());
    let mut machine = cpu_v3::CpuV3Sim::default();
    machine.load_program(0, &program.words).unwrap();
    machine.attach_device(2, Box::new(EchoDevice([0; 16])));
    let signal = match machine.run(10_000).unwrap() {
        cpu_v3::RunOutcome::Halted { signal, .. } => signal,
        outcome => panic!("CpuV3 program did not halt: {outcome:?}"),
    };
    assert_eq!(signal, 0x1234);
    assert_eq!(machine.memory(0xff23), 0);
    assert_eq!(machine.device::<EchoDevice>(2).unwrap().0[3], 0x1234);
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
fn device_indices_above_seven_are_rejected() {
    let result = std::panic::catch_unwind(|| {
        compile(
                "const INVALID_DEVICE: u16 = 8;\nconst TEST_CHANNEL: u16 = 0;\nfn main() { dev_send(INVALID_DEVICE, TEST_CHANNEL, 0); halt(0); }",
                CompilerOptions::default(),
            )
    });
    let error = result.expect_err("CpuV3 v0.6 supports only eight devices");
    let message = error
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| error.downcast_ref::<&str>().copied())
        .unwrap_or("");
    assert!(message.contains("device index 8"), "{message}");
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
            fn sh(a: u16, n: u16) -> u16 { a << n }
            fn lsr(a: u16, n: u16) -> u16 { a >> n }
            fn asr(a: i16, n: u16) -> i16 { a >> n }
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
            fn sh(a: u16) -> u16 { a << 2 }
            fn shr(a: u16) -> u16 { a >> 1 }
            fn asr(a: i16) -> i16 { a >> 1 }
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
            fn calc(x: u16) -> u16 {
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
fn destructive_operations_copy_the_source_when_both_remain_live() {
    // `a` is live across the shift and multiply, so the destructive forms
    // cannot reuse its register and must insert a MOV first.
    let source = r#"
            fn mix(x: u16) -> u16 {
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
fn if_value_arms_with_one_simple_instruction_use_a_conditional_move() {
    // `c3 - 1` is a single pure instruction, so the else arm stays
    // if-convertible; the countdown wraps 0 -> 2.
    let source = r#"
            fn countdown(c3: u16) -> u16 {
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
fn multiply_lowers_to_hardware_windows_and_muli() {
    let source = r#"
            fn products(a: u16, b: u16) -> u16 {
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

#[test]
fn byte_sign_extend_and_leading_zero_count_use_their_instructions() {
    let source = r#"
            fn extend(x: u16) -> i16 { sextb(x) }
            fn bits(x: u16) -> u16 { clz(x) }
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

#[test]
fn halt_and_signal_use_the_signal_encoding() {
    let source = r#"
            fn main() {
                signal(1, 5);
                signal(15, 6);
                halt(7);
            }
        "#;
    let program = compile(source, CompilerOptions::default());
    let signals = mnemonics(&program)
        .iter()
        .filter(|m| m.starts_with("signal"))
        .count();
    assert_eq!(signals, 2, "{}", program.listing);
    assert!(has_prefix(&program, "halt"), "{}", program.listing);
    assert_eq!(run(source), 7);
}

#[test]
fn special_register_reads_lower_to_mfsr() {
    let source = r#"
            fn main() {
                let c = read_cseg();
                let d = read_dseg();
                signal(1, c + d);
                halt(c + d);
            }
        "#;
    let program = compile(source, CompilerOptions::default());
    assert_eq!(
        mnemonics(&program)
            .iter()
            .filter(|m| m.starts_with("mfsr"))
            .count(),
        2,
        "{}",
        program.listing
    );
    assert_eq!(run(source), 0);
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

#[test]
fn invalid_constant_arguments_are_source_diagnostics() {
    // Invalid device/channel constants are rejected by the frontend with a
    // source diagnostic instead of reaching the lowering asserts.
    for (source, expected) in [
        (
            "const D: u16 = 8; fn main() { dev_recv(D, 0); halt(0); }",
            "device index 8",
        ),
        (
            "const C: u16 = 20; fn main() { dev_recv(0, C); halt(0); }",
            "channel 20",
        ),
        ("fn main() { dev_send(0, 16, 1); halt(0); }", "channel 16"),
    ] {
        let error = parse_source_with(source, 0)
            .err()
            .expect("the invalid constant must be rejected");
        assert!(
            error.to_string().contains(expected),
            "expected {expected:?} in:\n{error}"
        );
    }
}

#[test]
fn fallible_compile_succeeds_for_valid_programs() {
    let options = CompilerOptions::default();
    let program = parse_source_with("fn main() { halt(1); }", options.data_base).unwrap();
    let compiled =
        super::try_compile(program, &options, "main").expect("a well-formed program compiles");
    assert_eq!(compiled.words.last(), Some(&cpu_v3::halt()));
    assert_eq!(
        validate_program(&compiled.words, compiled.code_base),
        Ok(())
    );
}

#[test]
fn backend_error_display_is_a_compiler_message() {
    assert_eq!(
        BackendError::Validation(ProgramValidationError::InvalidInstruction {
            address: 0x10,
            word: 0xe800,
        })
        .to_string(),
        "illegal instruction 0xe800 at 0x0010"
    );
    assert_eq!(
        BackendError::UnknownFunction("ghost".to_string()).to_string(),
        "unknown function `ghost`"
    );
}

#[test]
fn program_validator_accepts_compiled_output() {
    let source = r#"
            fn main() {
                let a: u16 = 0x1234;
                let b = a + 5;
                signal(1, b);
                halt(b);
            }
        "#;
    let program = compile(source, CompilerOptions::default());
    assert_eq!(validate_program(&program.words, program.code_base), Ok(()));
}

#[test]
fn program_validator_rejects_illegal_and_malformed_images() {
    // the reserved revision-0.7 HALT word
    assert_eq!(
        validate_program(&[0xe800], 0),
        Err(ProgramValidationError::InvalidInstruction {
            address: 0,
            word: 0xe800
        })
    );
    // a prefix followed by a non-consumer, and a trailing prefix
    assert_eq!(
        validate_program(&[cpu_v3::prefix12(0), cpu_v3::halt()], 0),
        Err(ProgramValidationError::DanglingPrefix {
            address: 0,
            next: Some(cpu_v3::halt())
        })
    );
    assert_eq!(
        validate_program(&[cpu_v3::prefix12(0)], 0),
        Err(ProgramValidationError::DanglingPrefix {
            address: 0,
            next: None
        })
    );
    // relative consumers must zero payload12[11:8]
    assert_eq!(
        validate_program(
            &[
                cpu_v3::prefix12(0x100),
                cpu_v3::branch(cpu_v3::TestCondition::Equal, 0)
            ],
            0
        ),
        Err(ProgramValidationError::RelativePrefixHighBits {
            address: 0,
            payload: 0x100
        })
    );
    assert_eq!(
        validate_program(
            &[
                cpu_v3::prefix12(0x001),
                cpu_v3::branch(cpu_v3::TestCondition::Equal, 0)
            ],
            0
        ),
        Ok(())
    );
}
