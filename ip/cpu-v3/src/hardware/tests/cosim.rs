use super::*;

#[test]
#[ignore = "explicit external simulation of the reusable CpuV3 core"]
fn verify_verilog_with_iverilog() {
    digital_design_hardware::verify_verilog_with_iverilog::<CpuV3Core>().unwrap();
}

#[test]
fn core_cosim_directory_collision_preserves_existing_replay() {
    let next = AtomicUsize::new(0);
    let old = create_core_cosim_directory(&next).unwrap();
    let sentinel = old.join("sentinel.txt");
    std::fs::write(&sentinel, b"previous replay must survive").unwrap();

    // Simulate a later process reusing the same PID and directory sequence.
    // This tests directory allocation only; it does not run an RTL simulator.
    let reused = AtomicUsize::new(next.load(Ordering::Relaxed) - 1);
    let new = create_core_cosim_directory(&reused).unwrap();
    assert_ne!(new, old);
    assert_eq!(
        std::fs::read(sentinel).unwrap(),
        b"previous replay must survive"
    );
    assert_eq!(std::fs::read_dir(old).unwrap().count(), 1);
    assert_eq!(std::fs::read_dir(new).unwrap().count(), 0);
}

#[test]
#[ignore = "bounded RTL replay retention after success and a deliberate comparison failure"]
fn core_cosim_retains_replay_after_comparison() {
    let program = [cpu_v3::halt()];
    let emu = run_core_emu_trace(&program, 200);
    let tb = format!(
        "// Replay retention regression\n{}",
        build_core_cosim_tb(&program, &CpuV3Core::verilog_identity().module_name(), 200)
    );
    let rtl = run_core_rtl_trace(&tb);
    let target = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
    let prefix = format!("core-cosim-{}-", std::process::id());
    let directory = std::fs::read_dir(target)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(&prefix)
                && std::fs::read_to_string(path.join("tb.v")).is_ok_and(|saved| saved == tb)
        })
        .expect("the helper must retain its replay before returning the trace");
    assert_eq!(rtl.len(), emu.len());
    for (expected, actual) in emu.iter().zip(&rtl) {
        assert!(actual.equal_core(expected), "{actual:?} vs {expected:?}");
    }
    assert!(rtl.last().unwrap().halted);

    // This is an intentional comparator failure, not a failed RTL simulation.
    let mut wrong = *emu.last().unwrap();
    wrong.halt_signal ^= 1;
    let failure = std::panic::catch_unwind(|| {
        assert!(
            rtl.last().unwrap().equal_core(&wrong),
            "deliberately mismatched halt signal"
        );
    });
    assert!(failure.is_err());
    assert_eq!(std::fs::read_to_string(directory.join("tb.v")).unwrap(), tb);
    assert!(directory.join("sim.vvp").is_file());
    let mut sources = Vec::new();
    collect_verilog_files(&directory, &mut sources);
    assert!(
        !sources.is_empty(),
        "retain the generated core dependencies"
    );
    eprintln!(
        "Retained replay after both comparisons: {}",
        directory.display()
    );
}

#[test]
fn core_cosim_program_halts_in_the_simulators() {
    // The ignored emu/RTL co-simulation only compares well-defined runs, so
    // keep its program valid, in-range, and terminating. This checks the
    // program against the architectural oracle and the Rust cycle model
    // (the emu half of the co-sim) without needing Icarus, and pins the
    // cycle count below the emu trace cap used by the co-sim.
    let program = core_cosim_program();
    let mut machine = CpuV3Sim::with_physical_memory_words(1 << 16);
    machine.load_program(0, &program).unwrap();
    assert!(matches!(machine.run(10_000), Ok(RunOutcome::Halted { .. })));

    let emu = run_core_emu_trace(&program, 10_000);
    let last = emu.last().copied().expect("emu trace empty");
    assert!(!last.fault, "co-sim program faulted in the cycle model");
    assert!(
        last.halted,
        "co-sim program did not halt in the cycle model"
    );
    assert!(
        emu.len() < 2000,
        "co-sim program needs {} cycles, above the co-sim emu trace cap",
        emu.len()
    );
}

#[test]
#[ignore = "bounded emulator-vs-Icarus execution of affine resource cleanup and transfer"]
fn core_emu_matches_rtl_compiled_resource_scope_program() {
    for (label, program) in [
        (
            "lexical resource",
            crate::rcc_backend::resource_tests::protocol_program(),
        ),
        (
            "graphics prerequisites",
            crate::rcc_backend::rustc_tests::graphics_prerequisites_program(false),
        ),
        (
            "graphics debug profile",
            crate::rcc_backend::rustc_tests::graphics_prerequisites_program(true),
        ),
    ] {
        let emu = run_core_emu_trace(&program, 30_000);
        let last = emu.last().expect("nonempty resource trace");
        assert!(last.halted && !last.fault, "{label}: {last:?}");
        assert_eq!(last.halt_signal, 0x600d);
        // Like the architectural/cycle model and normal boot ZERO initialization,
        // this fixture starts ordinary data as zero (including the event cursor).
        let tb = build_core_cosim_tb(
        &program,
        &CpuV3Core::verilog_identity().module_name(),
        emu.len() + 100,
    )
    .replacen(
        "initial begin\n",
        "initial begin\n    for (index = 0; index < 65536; index = index + 1) memory[index] = 0;\n",
        1,
    );
        let rtl = run_core_rtl_trace(&tb);
        assert_eq!(rtl.len(), emu.len());
        for (cycle, (expected, actual)) in emu.iter().zip(&rtl).enumerate() {
            assert!(
                actual.equal_core(expected),
                "{label} trace differs at cycle {cycle}: {actual:?} vs {expected:?}"
            );
        }
    }
}

/// FPU v2 back-to-back FLD after an async store: the reproducer for the
/// one-cycle request slip seen at system level.
#[test]
#[ignore = "explicit emulator-vs-Icarus co-simulation of FPU v2 memory beats"]
fn core_emu_matches_rtl_fpu_ldst() {
    // r1=0x0100, r2=0x0102; store r0=7 to [r1]; FLD f1,[r1]; FLD f2,[r2]; HALT
    let program = vec![
        0xf010, 0xa310, // r1 = 0x0100
        0xf010, 0xa322, // r2 = 0x0102
        0xa007, // ADDI r0, 7
        0x9010, // STORE r0, [r1] (async store buffer)
        0xe100, 0x0400, // FLD f1, [r1]
        0xe200, 0x0800, // FLD f2, [r2]
        0xd041, 0x0c20, // MUL f3 = f1 * f2
        0xc042, 0x1018, // VMULS.2 f4..f5 = f1..f2 * f2
        0xc042, 0x0068, // DOT.2 ACC = f1*f1 + f2*f2 = 1 + 4 = 5.0
        0xc042, 0x0070, // DOTADD.2: ACC = 10.0
        0xc042, 0x1878, // DOTSTORE.2 f6 = 5.0, ACC cleared
        0x6c00, // HALT
    ];
    let module_name = CpuV3Core::verilog_identity().module_name();
    let emu = run_core_emu_trace(&program, 2000);
    let max_cycles = emu.len() + 400;
    let tb = build_core_cosim_tb(&program, &module_name, max_cycles);
    let rtl = run_core_rtl_trace(&tb);
    if emu.len() != rtl.len() {
        let dump = |trace: &[CoreCosimOut]| {
            trace
                .iter()
                .enumerate()
                .map(|(i, v)| {
                    format!(
                        "{i}: pc={} retired={} ireq={} iaddr={:#06x} dreq={} dw={} daddr={:#06x}",
                        v.pc,
                        v.retired_words,
                        v.instruction_request_valid,
                        v.instruction_address,
                        v.data_request_valid,
                        v.data_write,
                        v.data_address
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        };
        panic!(
            "length mismatch emu={} rtl={}\n--- emu ---\n{}\n--- rtl ---\n{}",
            emu.len(),
            rtl.len(),
            dump(&emu),
            dump(&rtl)
        );
    }
    for (index, (expected, actual)) in emu.iter().zip(&rtl).enumerate() {
        assert!(
            actual.equal_core(expected),
            "mismatch at cycle {index}\nemu={expected:?}\nrtl={actual:?}"
        );
    }
    assert!(emu.last().copied().expect("emu trace empty").halted);
}

#[test]
#[ignore = "explicit emulator-vs-Icarus co-simulation of a compiled scalar FPU program"]
fn core_emu_matches_rtl_compiled_scalar_fpu_program() {
    // The rcc scalar `fix32` path end to end on the hardware-supported subset:
    // integer construction (`I16TOF`), calls through the F-register ABI, an
    // FPU `ADD`/`MUL`/`trunc`, the raw-half bridge (`ILO2F`/`IHI2F`), a scalar
    // `CMP` feeding a GPR branch, the signed numeric conversion back
    // (`FTOI16`, truncating toward zero), and values kept live across a call so
    // they spill through FLD/FST:
    // a=7, b=-2, addfix(a,b)=5, scaled=-14, kept=1.5, down=1, pick=1,
    // live=-13, (5 + -13 + 0.5).to_int() = -7 -> 0xfff9.
    let source = r#"
        fn addfix(a: fix32, b: fix32) -> fix32 { a + b }
        fn main() {
            let a = fix32::from_int(7);
            let b = fix32::from_int(-2);
            let s = addfix(a, b);
            let scaled = a * b;
            let kept = fix32::from_words(0x8000, 0x0001);
            let down = kept.trunc();
            let negative = fix32::from_words(0x8000, 0xfffe); // -1.5
            if kept.floor().to_int() != 1 || kept.ceil().to_int() != 2
                || kept.round().to_int() != 2 || kept.trunc().to_int() != 1
                || negative.floor().to_int() != -2 || negative.ceil().to_int() != -1
                || negative.round().to_int() != -1 || negative.trunc().to_int() != -1 {
                halt(0xdead);
            }
            let pick = if s < down { s } else { down };
            let live = addfix(scaled, pick);
            halt((s + live + (kept - down)).to_int() as u16);
        }
    "#;
    let program = compile(source);
    let module_name = CpuV3Core::verilog_identity().module_name();
    let emu = run_core_emu_trace(&program, 4000);
    assert!(!emu.is_empty(), "emu trace empty");
    let last_emu = emu.last().copied().expect("emu trace non-empty");
    assert!(
        !last_emu.fault,
        "compiled FPU program faulted in the cycle model: code={} pc={:#06x}",
        last_emu.fault_code, last_emu.fault_pc
    );
    assert!(last_emu.halted, "compiled FPU program did not halt");
    assert_eq!(last_emu.halt_signal, 0xfff9, "unexpected Q16.16 result");

    let max_cycles = emu.len() + 600;
    let tb = build_core_cosim_tb(&program, &module_name, max_cycles);
    let rtl = run_core_rtl_trace(&tb);
    // Report the first divergence (if any) before the length check, so a timing
    // slip points at the exact cycle.
    for (index, (expected, actual)) in emu.iter().zip(&rtl).enumerate() {
        if !actual.equal_core(expected) {
            panic!("mismatch at cycle {index}\nemu={expected:?}\nrtl={actual:?}");
        }
    }
    assert_eq!(
        emu.len(),
        rtl.len(),
        "emu/RTL trace length mismatch: emu={} rtl={}",
        emu.len(),
        rtl.len()
    );
    assert_eq!(
        rtl.last().copied().expect("rtl trace empty").halt_signal,
        0xfff9
    );
}

#[test]
#[ignore = "explicit emulator-vs-Icarus co-simulation of a compiled vector FPU program"]
fn core_emu_matches_rtl_compiled_vector_fpu_program() {
    // The rcc vector path end to end on the hardware-supported subset: vec4
    // construction, an ABI-packed vector call, VADD, VMULS scalar broadcast,
    // lane extraction, `fdot`, and a vector value live across a call so it
    // spills through FLDV/FSTV:
    // a=(1,2,3,4), b=(10,20,30,40), add4(a,b)=(11,22,33,44),
    // d = c*2 = (22,44,66,88), s = 220, fdot(c,d) = 7260 -> 7480.
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
    let program = compile(source);
    let module_name = CpuV3Core::verilog_identity().module_name();
    let emu = run_core_emu_trace(&program, 6000);
    assert!(!emu.is_empty(), "emu trace empty");
    let last_emu = emu.last().copied().expect("emu trace non-empty");
    assert!(
        !last_emu.fault,
        "compiled vector FPU program faulted in the cycle model: code={} pc={:#06x}",
        last_emu.fault_code, last_emu.fault_pc
    );
    assert!(last_emu.halted, "compiled vector FPU program did not halt");
    assert_eq!(last_emu.halt_signal, 7480, "unexpected vector result");

    let max_cycles = emu.len() + 600;
    let tb = build_core_cosim_tb(&program, &module_name, max_cycles);
    let rtl = run_core_rtl_trace(&tb);
    for (index, (expected, actual)) in emu.iter().zip(&rtl).enumerate() {
        if !actual.equal_core(expected) {
            panic!("mismatch at cycle {index}\nemu={expected:?}\nrtl={actual:?}");
        }
    }
    assert_eq!(
        emu.len(),
        rtl.len(),
        "emu/RTL trace length mismatch: emu={} rtl={}",
        emu.len(),
        rtl.len()
    );
    assert_eq!(
        rtl.last().copied().expect("rtl trace empty").halt_signal,
        7480
    );
}

#[test]
#[ignore = "explicit emulator-vs-Icarus co-simulation of a compiled special-function FPU program"]
fn core_emu_matches_rtl_compiled_special_fpu_program() {
    // The rcc C3 special-function path end to end: `frcp`, `frsqrt`, `fsin`,
    // `fcos` and the dual-output `fsincos` lower to the SCALAR special subops
    // and their results flow back through the raw-half bridge into a mixed halt
    // signal, so the RTL's hidden-BSRAM LUT path and the stylized trace must
    // match the emulator cycle for cycle.
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
    let program = compile(source);
    let module_name = CpuV3Core::verilog_identity().module_name();
    let emu = run_core_emu_trace(&program, 6000);
    assert!(!emu.is_empty(), "emu trace empty");
    let last_emu = emu.last().copied().expect("emu trace non-empty");
    assert!(
        !last_emu.fault,
        "compiled special FPU program faulted in the cycle model: code={} pc={:#06x}",
        last_emu.fault_code, last_emu.fault_pc
    );
    assert!(last_emu.halted, "compiled special FPU program did not halt");
    let x = 3 << 16;
    let (sin, cos) = crate::sincos_q16(x);
    let expected = (crate::rcp_q16(x) as u16)
        ^ (((crate::rsqrt_q16(x) as u32 >> 16) as u16) << 1)
        ^ (((sin as u32 >> 16) as u16) << 2)
        ^ (((cos as u32 >> 16) as u16) << 3)
        ^ ((sin as u16) << 4)
        ^ (((cos as u32 >> 16) as u16) << 5);
    assert_eq!(
        last_emu.halt_signal, expected,
        "unexpected special-function result"
    );

    let max_cycles = emu.len() + 600;
    let tb = build_core_cosim_tb(&program, &module_name, max_cycles);
    let rtl = run_core_rtl_trace(&tb);
    for (index, (expected, actual)) in emu.iter().zip(&rtl).enumerate() {
        if !actual.equal_core(expected) {
            panic!("mismatch at cycle {index}\nemu={expected:?}\nrtl={actual:?}");
        }
    }
    assert_eq!(
        emu.len(),
        rtl.len(),
        "emu/RTL trace length mismatch: emu={} rtl={}",
        emu.len(),
        rtl.len()
    );
    assert_eq!(
        rtl.last().copied().expect("rtl trace empty").halt_signal,
        expected
    );
}

#[test]
#[ignore = "explicit emulator-vs-Icarus co-simulation of the CpuV3 core pipeline"]
fn core_emu_matches_rtl_pipeline_overlap() {
    let program = core_cosim_program();
    let module_name = CpuV3Core::verilog_identity().module_name();
    let emu = run_core_emu_trace(&program, 2000);
    // A dependent-immediate chain plus a device handshake need a bounded
    // but generous window.
    let max_cycles = emu.len() + 400;
    let tb = build_core_cosim_tb(&program, &module_name, max_cycles);
    let rtl = run_core_rtl_trace(&tb);
    assert!(
        emu.len() == rtl.len(),
        "emu/RTL trace length mismatch: emu={} rtl={}\nemu={:?}\nrtl={:?}",
        emu.len(),
        rtl.len(),
        emu.iter().map(|v| v.pc).collect::<Vec<_>>(),
        rtl.iter().map(|v| v.pc).collect::<Vec<_>>()
    );
    for (index, (expected, actual)) in emu.iter().zip(&rtl).enumerate() {
        assert!(
            actual.equal_core(expected),
            "emu/RTL core mismatch at cycle {index}\nemu={expected:?}\nrtl={actual:?}"
        );
    }
    let last_emu = emu.last().copied().expect("emu trace empty");
    assert!(last_emu.halted, "core co-sim must end in halt");
}

#[test]
#[ignore = "bounded reserved cache-control encoding emulator/RTL comparison"]
fn core_emu_matches_rtl_reserved_cache_encodings() {
    let module_name = CpuV3Core::verilog_identity().module_name();
    for word in (0..16)
        .map(|n| 0x6e70 | n)
        .chain((1..16).map(|n| 0x6e80 | n))
        .chain([0x2700, 0x2712, 0x27ff])
    {
        assert_eq!(cpu_v3::decode(word), cpu_v3::Instruction::Invalid { word });
        let program = [word, cpu_v3::halt()];
        let emu = run_core_emu_trace(&program, 200);
        let rtl = run_core_rtl_trace(&build_core_cosim_tb(&program, &module_name, 200));
        assert_eq!(emu.len(), rtl.len(), "encoding {word:04x} trace length");
        for (e, r) in emu.iter().zip(&rtl) {
            assert!(
                r.equal_core(e),
                "encoding {word:04x}: expected {e:?}, got {r:?}"
            );
        }
        let end = rtl.last().unwrap();
        assert!(end.fault && !end.halted, "encoding {word:04x} must fault");
        assert_eq!(end.fault_code, CPU_V3_FAULT_INVALID_INSTRUCTION);
        assert_eq!(end.fault_pc, 0);
        assert_eq!(end.retired_words, 0);
    }
}

#[test]
#[ignore = "bounded FST maintenance barrier and prefix emulator/RTL comparison"]
fn core_emu_matches_rtl_maintenance_barrier_prefix() {
    let mut p = cpu_v3::load_immediate16(1, 0x4000).to_vec();
    for lane in 0..4 {
        for subop in [2, 3] {
            p.extend(cpu_v3::fpu_aux_raw(
                cpu_v3::FpuAuxKind::IntegerRegister,
                0,
                0,
                lane,
                subop,
                0,
            ));
        }
    }
    p.extend(cpu_v3::fpu_aux_raw(
        cpu_v3::FpuAuxKind::IntegerRegister,
        1,
        0,
        0,
        1,
        3,
    ));
    p.push(0xf123); // Unused prefix expires while the maintenance write waits.
    p.push(cpu_v3::device_send(0, 0, 4));
    p.push(cpu_v3::halt());
    let emu = run_core_emu_trace(&p, 2000);
    let rtl = run_core_rtl_trace(&build_core_cosim_tb(
        &p,
        &CpuV3Core::verilog_identity().module_name(),
        2000,
    ));
    assert_eq!(emu.len(), rtl.len());
    for (e, r) in emu.iter().zip(&rtl) {
        assert!(
            r.equal_core(e),
            "maintenance prefix: expected {e:?}, got {r:?}"
        );
    }
    assert!(rtl.last().unwrap().halted);
}

#[test]
#[ignore = "bounded scalar/vector FST snapshots, source reuse and capture holds"]
fn core_emu_matches_rtl_fst_snapshot_lifetime() {
    let values = [0x1234_5678u32, 0x8765_4321, 0x9abc_def0, 0x5a5a_a5a5];
    for lanes in 1..=4u8 {
        let mut p = Vec::new();
        p.extend(cpu_v3::load_immediate16(1, 0x0100));
        p.extend(cpu_v3::load_immediate16(2, 0x0200));
        for (lane, value) in values.into_iter().enumerate() {
            for (part, subop) in [(value as u16, 2), ((value >> 16) as u16, 3)] {
                p.extend(cpu_v3::load_immediate16(0, part));
                p.extend(cpu_v3::fpu_aux_raw(
                    cpu_v3::FpuAuxKind::IntegerRegister,
                    0,
                    0,
                    lane as u8,
                    subop,
                    0,
                ));
            }
        }
        let memory = |x, fa, fd, subop| {
            cpu_v3::fpu_aux_raw(
                cpu_v3::FpuAuxKind::IntegerRegister,
                x,
                fa,
                fd,
                subop,
                lanes - 1,
            )
        };
        p.extend(memory(1, 0, 0, 1));
        // Reuse the original RF sources while their captured values drain.
        p.extend(cpu_v3::fpu_vector(
            0,
            0,
            0,
            cpu_v3::FpuVectorLength::Vec4,
            cpu_v3::FpuVectorSubop::VAdd,
            0,
        ));
        p.extend(memory(1, 0, 8, 0));
        p.extend(memory(2, 8, 0, 1));
        p.push(cpu_v3::halt());
        let emu = run_core_emu_trace(&p, 2000);
        let tb = build_core_cosim_tb(&p, &CpuV3Core::verilog_identity().module_name(), 2000);
        let rtl = run_core_rtl_trace(&tb);
        assert_eq!(emu.len(), rtl.len());
        for (e, r) in emu.iter().zip(&rtl) {
            assert!(r.equal_core(e), "FST{lanes}: expected {e:?}, got {r:?}");
        }
        let writes = |trace: &[CoreCosimOut]| {
            trace
                .iter()
                .filter(|r| r.data_request_valid && r.data_write)
                .map(|r| (r.data_address, r.data_write_data))
                .collect::<Vec<_>>()
        };
        let expected = [0x0100u32, 0x0200]
            .into_iter()
            .flat_map(|base| {
                values[..usize::from(lanes)]
                    .iter()
                    .flat_map(|v| [*v as u16, (*v >> 16) as u16])
                    .enumerate()
                    .map(move |(i, v)| (base + i as u32, v))
            })
            .collect::<Vec<_>>();
        assert_eq!(writes(&rtl), expected);
        assert!(rtl.last().unwrap().halted);
        for capture_beat in [0, lanes - 1] {
            let monitor = format!(
                "reg paused=0;integer pause_left=0;\n\
                 always @(negedge clk)begin\n\
                  if(pause_left>0)begin pause_left=pause_left-1;hold=pause_left!=0;end\n\
                  else if(!reset&&!paused&&dut.state==13&&dut.fpu2_beat=={capture_beat})begin\n\
                   paused=1;pause_left=3;hold=1;end else hold=0;end\n"
            );
            let held = run_core_rtl_trace(&tb.replacen("endmodule", &(monitor + "endmodule"), 1));
            assert!(held.last().unwrap().halted);
            assert_eq!(writes(&held), expected, "FST{lanes} hold at {capture_beat}");
        }
    }
}

#[test]
#[ignore = "bounded FPU word-pulse and completion ownership across core hold"]
fn core_fpu_word_and_completion_survive_hold() {
    let mut p = Vec::new();
    for (lane, value) in [0x00018000u32, 0xffff4000, 0x00020000, 0x00008000]
        .into_iter()
        .enumerate()
    {
        for (half, subop) in [(value as u16, 2), ((value >> 16) as u16, 3)] {
            p.extend(cpu_v3::load_immediate16(0, half));
            p.extend(cpu_v3::fpu_aux_raw(
                cpu_v3::FpuAuxKind::IntegerRegister,
                0,
                0,
                lane as u8,
                subop,
                0,
            ));
        }
    }
    // Same-base broadcast is the source lifetime most affected by the new pipe.
    p.extend([0xc000, 0x0218]); // VMULS4 f0, f0, f0
    p.extend([0xd000, (8 << 10) | (12 << 4)]); // RCP f8, f0
    p.extend([0xd000, (9 << 10) | (13 << 4)]); // RSQRT f9, f0
    p.extend([0xd000, (10 << 10) | (14 << 4)]); // SINCOS f10/f11, f0
    p.extend(cpu_v3::load_immediate16(1, 0x0100));
    for fa in [0u8, 8] {
        p.extend(cpu_v3::fpu_aux_raw(
            cpu_v3::FpuAuxKind::IntegerRegister,
            1,
            fa,
            0,
            1,
            3,
        ));
        p.extend(cpu_v3::load_immediate16(1, 0x0200));
    }
    p.push(cpu_v3::halt());
    let tb = build_core_cosim_tb(&p, &CpuV3Core::verilog_identity().module_name(), 4000);
    let reference = run_core_rtl_trace(&tb);
    assert!(reference.last().unwrap().halted);
    let writes = |trace: &[CoreCosimOut]| {
        trace
            .iter()
            .filter(|r| r.data_request_valid && r.data_write)
            .map(|r| (r.data_address, r.data_write_data))
            .collect::<Vec<_>>()
    };
    let expected = writes(&reference);
    for state in [9, 10, 13] {
        let monitor = format!(
            "reg[15:0] paused_pc=16'hffff;integer pause_left=0;\n\
             always @(negedge clk)begin\n\
              if(pause_left>0)begin pause_left=pause_left-1;hold=pause_left!=0;end\n\
              else if(!reset&&dut.state=={state}&&paused_pc!=dut.instruction_pc)begin\n\
               paused_pc=dut.instruction_pc;pause_left=6;hold=1;end else hold=0;end\n"
        );
        let held = run_core_rtl_trace(&tb.replacen("endmodule", &(monitor + "endmodule"), 1));
        assert!(
            held.last().unwrap().halted,
            "hold in state {state} lost completion"
        );
        assert_eq!(
            writes(&held),
            expected,
            "hold in state {state} changed FPU data"
        );
    }
    for period in [17, 19] {
        let emu =
            run_core_emu_trace_with_hold(&p, 4000, |cycle| (5..11).contains(&(cycle % period)));
        let anchor = "while (cycles < 4000 && !end_flag) begin\n";
        assert!(tb.contains(anchor));
        let mut held_tb = tb.replace(
            anchor,
            &format!("{anchor}#1;hold = started && hold_cycle%{period}>=5 && hold_cycle%{period}<11;#1;\n"),
        );
        held_tb = held_tb.replace("reg hold = 0;", "reg hold = 0; integer hold_cycle=0;");
        held_tb = held_tb.replace(
            "cycles = cycles + 1;\n",
            "cycles = cycles + 1; if(started)hold_cycle=hold_cycle+1;\n",
        );
        held_tb = held_tb.replace(
            "always @(posedge clk) begin\n",
            "always @(posedge clk) if(!hold) begin\n",
        );
        let rtl = run_core_rtl_trace(&held_tb);
        for (index, (e, r)) in emu.iter().zip(&rtl).enumerate() {
            assert!(
                r.equal_core(e),
                "periodic hold {period} sample {index}: emu={e:?}, rtl={r:?}"
            );
        }
        assert_eq!(emu.len(), rtl.len(), "periodic hold length {period}");
        assert!(rtl.last().unwrap().halted);
    }
}

#[test]
#[ignore = "bounded dual-address LCOPY core sideband, mapping and fault co-simulation"]
fn core_emu_matches_rtl_dual_address_copy_commands() {
    for (source, destination, destination_page) in [
        (0x4020, 0x8020, 0x34),
        (0x4021, 0x8020, 0x34),
        (0x4020, 0x8021, 0x34),
        (0x4020, 0x8020, 0x12),
    ] {
        let mut p = cpu_v3::load_immediate16(4, 0x12).to_vec();
        p.push(cpu_v3::write_data_segment_page(1, 4));
        p.extend(cpu_v3::load_immediate16(4, destination_page));
        p.push(cpu_v3::write_data_segment_page(2, 4));
        p.extend(cpu_v3::load_immediate16(1, source));
        p.extend(cpu_v3::load_immediate16(2, destination));
        p.extend([
            cpu_v3::line_copy(1, 2),
            cpu_v3::alu(AluOp::Xor, 3, 1, 2),
            cpu_v3::signal(3, 0),
        ]);
        let emu = run_core_emu_trace(&p, 400);
        let mut tb = build_core_cosim_tb(&p, &CpuV3Core::verilog_identity().module_name(), 400);
        let expected = usize::from((source | destination) & 15 == 0);
        let destination_physical = (u32::from(destination_page) << 14) | 0x20;
        let check = format!(
            r#"integer copy_count=0;
            always @(posedge clk) if(!reset) begin
                if(data_line_copy_valid) begin
                    if(data_line_clean_valid || data_line_copy_source!==22'h48020 || data_line_clean_address!==22'h{destination_physical:x})
                        $fatal(1,"dual-address copy mapped sideband mismatch");
                    copy_count=copy_count+1;
                end
                if((halted || fault) && copy_count!={expected}) $fatal(1,"copy command count mismatch");
            end
        "#
        );
        let i = tb.rfind("endmodule").unwrap();
        tb.insert_str(i, &check);
        let rtl = run_core_rtl_trace(&tb);
        assert_eq!(emu.len(), rtl.len());
        for (a, b) in emu.iter().zip(&rtl) {
            assert!(a.equal_core(b), "expected {a:?}, got {b:?}");
        }
        if expected == 0 {
            assert_eq!(rtl.last().unwrap().fault_code, CPU_V3_FAULT_DATA_MEMORY);
        } else {
            assert_eq!(rtl.last().unwrap().halt_signal, source ^ destination);
        }
    }
}
