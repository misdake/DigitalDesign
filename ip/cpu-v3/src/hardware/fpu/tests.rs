use super::*;

#[test]
#[ignore = "shared FPU port/writeback replay with aliasing, CE and each abort age"]
fn verify_port_owner_emu_matches_rtl() {
    use std::fmt::Write;
    fn idle() -> CpuV3FpuInputValue {
        CpuV3FpuInputValue {
            abort: false,
            word_valid: false,
            word: 0,
            ext_access: false,
            ext_write_enable: false,
            ext_write_address: 0,
            ext_write_data: 0,
            ext_read_address: 0,
            integer_in_valid: false,
            integer_in_a: 0,
            integer_in_b: 0,
        }
    }
    fn record(
        state: &mut CpuV3FpuState,
        tb: &mut String,
        count: &mut usize,
        input: CpuV3FpuInputValue,
        enabled: bool,
    ) {
        assert!(*count < 20000, "product emu replay cycle limit");
        for value in CpuV3FpuInput::verilog_values(&input) {
            writeln!(tb, "{}={}'h{:x};", value.name, value.width, value.value).unwrap();
        }
        tb.push_str("#1;\n");
        for value in CpuV3FpuOutput::verilog_values(&state.comb(&input)) {
            if value.name != "ext_read_data" || input.ext_access {
                writeln!(
                    tb,
                    "if({} !== {}'h{:x}) $fatal(1,\"emu output {} cycle={}\");",
                    value.name, value.width, value.value, value.name, *count
                )
                .unwrap();
            }
        }
        if enabled {
            tb.push_str("#3;clk=1;#1;\n");
            state.tick(&input);
        } else {
            tb.push_str("#4;\n");
        }
        writeln!(
            tb,
            "if(dut.acc_r !== 64'h{:x}) $fatal(1,\"emu ACC cycle={}\");",
            state.accumulator() as u64,
            *count
        )
        .unwrap();
        // Keep all 68 comparisons on every beat, without expanding 68
        // separate Verilog statements for every recorded cycle.
        let mut packed = String::with_capacity(68 * 8);
        for word in state.rf.mirror_a[..68].iter().rev() {
            write!(packed, "{word:08x}").unwrap();
        }
        writeln!(tb, "check_rf(2176'h{packed},{});", *count).unwrap();
        tb.push_str("#4;clk=0;\n");
        *count += 1;
    }
    let mut state = CpuV3FpuState::default();
    let mut tb = String::from("module tb;reg clk=0;\n");
    for value in CpuV3FpuInput::verilog_values(&idle()) {
        writeln!(tb, "reg [{}:0] {}=0;", value.width - 1, value.name).unwrap();
    }
    for value in CpuV3FpuOutput::verilog_values(&state.comb(&idle())) {
        writeln!(tb, "wire [{}:0] {};", value.width - 1, value.name).unwrap();
    }
    tb.push_str(
        r#"CpuV3Fpu dut(.*);
task check_rf;
 input [2175:0] expected;
 input integer cycle;
 integer index;
 begin
  for(index=0;index<68;index=index+1)
   if(dut.rf.mirror_0[index] !== expected[index*32+:32])
    $fatal(1,"emu RF%0d cycle=%0d",index,cycle);
 end
endtask
initial begin
"#,
    );
    let mut count = 0;
    for index in 0..68 {
        let mut value = idle();
        value.ext_access = true;
        value.ext_write_enable = true;
        value.ext_write_address = index;
        value.ext_write_data = match index % 4 {
            0 => 0x80000000,
            1 => 0x7fffffff,
            2 => 0xffff8000,
            _ => 0x13579bdf ^ (index * 0x271311),
        };
        record(&mut state, &mut tb, &mut count, value, true);
    }
    for mode in 0..30 {
        for abort_age in -1..12 {
            let len = ((abort_age + 1) % 4) as u16;
            let (opcode, fa, fb, word1) = if mode < 6 {
                let subop = match mode {
                    0 | 5 => 2,
                    1 => 3,
                    2 => 13,
                    3 => 14,
                    _ => 15,
                };
                let word1 = if mode == 5 {
                    61 << 10 | 2 << 4
                } else {
                    61 << 10 | len << 8 | subop << 3 | len
                };
                (if mode == 5 { 0xd } else { 0xc }, 62, 63, word1)
            } else if mode < 18 {
                let subop = [0, 1, 3, 4, 5, 6, 7, 8, 9, 10, 11, 15][mode - 6];
                (0xd, 62, 63, 61 << 10 | subop << 4)
            } else if mode < 25 {
                let subop = [0, 1, 4, 5, 6, 7, 12][mode - 18];
                (0xc, 59, 60, 60 << 10 | (len % 3) << 8 | subop << 3)
            } else {
                let (subop, special_mode) = match mode {
                    25 => (12, 0),
                    26 => (13, 0),
                    _ => (14, (mode - 27) as u16),
                };
                (0xd, 62, 63, 61 << 10 | subop << 4 | special_mode)
            };
            // Exercise normalization, zero, signed endpoints and fractional
            // inputs while retaining the independent cycle/numeric model.
            let values = [0, 1, 0xffff, 0x10000, 0x7fffffff, 0x80000000, 0xffff8000];
            let mut seed = idle();
            seed.ext_access = true;
            seed.ext_write_enable = true;
            seed.ext_write_address = 62;
            seed.ext_write_data = values[(mode + (abort_age + 1) as usize) % values.len()];
            record(&mut state, &mut tb, &mut count, seed, true);
            let mut input = idle();
            input.word_valid = true;
            input.word = opcode << 12 | fa << 6 | fb;
            record(&mut state, &mut tb, &mut count, input, true);
            let mut input = idle();
            input.word_valid = true;
            input.word = word1 as u64;
            record(&mut state, &mut tb, &mut count, input, true);
            for age in 0..15 {
                let mut input = idle();
                input.abort = age == abort_age;
                if age % 3 == 1 {
                    record(&mut state, &mut tb, &mut count, input.clone(), false);
                }
                record(&mut state, &mut tb, &mut count, input, true);
            }
            assert!(!state.comb(&idle()).busy);
        }
    }
    writeln!(tb,"$display(\"DIGITAL_DESIGN_PASS shared FPU ports emu cycles={count}\");$finish;end initial begin #{};$fatal(1,\"emu replay timeout\");end endmodule",count*10+100).unwrap();
    let project = digital_design_hardware::VerilogProject::generate::<CpuV3Fpu>().unwrap();
    let directory = std::env::temp_dir().join(format!("cpu-product-emu-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(
        directory.join("modules.v"),
        project.files.into_values().collect::<Vec<_>>().join("\n"),
    )
    .unwrap();
    std::fs::write(directory.join("tb.v"), tb).unwrap();
    let result = std::process::Command::new(
        std::env::var_os("IVERILOG_EXE").unwrap_or_else(|| "iverilog".into()),
    )
    .current_dir(&directory)
    .args([
        "-g2012",
        "-D__ICARUS__",
        "-s",
        "tb",
        "-o",
        "sim.vvp",
        "modules.v",
        "tb.v",
    ])
    .output()
    .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let result =
        std::process::Command::new(std::env::var_os("VVP_EXE").unwrap_or_else(|| "vvp".into()))
            .current_dir(&directory)
            .arg("sim.vvp")
            .output()
            .unwrap();
    assert!(
        result.status.success()
            && String::from_utf8_lossy(&result.stdout).contains("DIGITAL_DESIGN_PASS"),
        "{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout));
}

#[test]
#[ignore = "explicit external simulation of the unified FPU ALU lane/commit engine"]
fn verify_fpu_unified_alu_path_with_iverilog() {
    digital_design_hardware::verify_verilog_with_iverilog::<CpuV3FpuAluPath>().unwrap();
}

#[test]
#[ignore = "bounded selected RF, one-product DSP and special-function calendars"]
fn verify_selected_compute_leaves() {
    digital_design_hardware::verify_verilog_with_iverilog::<CpuV3FpuRegisterRam>().unwrap();
    digital_design_hardware::verify_verilog_with_iverilog::<execution::CpuV3FpuExecRf>().unwrap();
    digital_design_hardware::verify_verilog_with_iverilog::<execution::CpuV3FpuExecMul>().unwrap();
    digital_design_hardware::verify_verilog_with_iverilog::<execution::CpuV3FpuExecSpecial>()
        .unwrap();

    // Mutate the DUT, not the monitor: a permanently enabled B read must
    // fail the same held-Q assertion used by the positive execution RF.
    struct ForcedBRead;
    impl HardwareIdentity for ForcedBRead {
        const TARGET_RESOURCE_LEAF: bool = false;
        fn verilog_identity() -> VerilogIdentity {
            execution::CpuV3FpuExecRf::verilog_identity()
        }
    }
    impl Module for ForcedBRead {
        type Input = execution::CpuV3FpuExecRfInput;
        type Output = CpuV3FpuRegisterRamOutput;
        type EmuState = ();
        const USES_MAIN_CLOCK: bool = true;
        const EMU_AVAILABLE: bool = false;
        fn verilog_source() -> Option<String> {
            let source = execution::CpuV3FpuExecRf::verilog_source()?;
            assert_eq!(source.matches("if (read_b_enable)").count(), 1);
            Some(source.replace("if (read_b_enable)", "if (1'b1)"))
        }
        fn verilog_testbench() -> Option<String> {
            execution::CpuV3FpuExecRf::verilog_testbench()
        }
    }
    match digital_design_hardware::verify_verilog_with_iverilog::<ForcedBRead>() {
        Err(digital_design_hardware::VerilogSimulationError::MissingPassMarker {
            stdout, ..
        }) => assert!(
            stdout.contains("B hold while address/write change"),
            "{stdout}"
        ),
        other => panic!("forced B-read enable did not fail the hold check: {other:?}"),
    }
}

/// The architecture `encoding` module is the public source of truth for
/// every FPU v2 field. This module keeps a private copy for the Verilog
/// literals, so replay the whole 16-bit word space and require the two to
/// agree exactly. A silent second encoding can never survive this test.
#[test]
fn architecture_encoding_locks_the_private_rtl_extractor() {
    use crate::{
        fpu_aux_fa, fpu_aux_x, fpu_fa, fpu_fb, fpu_fd, fpu_mode, fpu_scalar_subop_field,
        fpu_vector_len_field, fpu_vector_subop_field, FpuAuxKind, FpuOpcode, FpuVectorLength,
    };
    for word in 0..=u16::MAX {
        let major = (word >> 12) as u8;
        assert_eq!(encoding::opcode(word), major, "opcode {word:#06x}");
        assert_eq!(
            FpuOpcode::from_word0(word).is_some(),
            matches!(major, 0xc..=0xe),
            "fpu opcode set {word:#06x}"
        );
        assert_eq!(encoding::word0_fa(word), fpu_fa(word), "{word:#06x}");
        assert_eq!(encoding::word0_fb(word), fpu_fb(word), "{word:#06x}");
        assert_eq!(encoding::aux_fa(word), fpu_aux_fa(word), "{word:#06x}");
        assert_eq!(encoding::aux_x(word), fpu_aux_x(word), "{word:#06x}");
        assert_eq!(
            encoding::aux_kind(word),
            FpuAuxKind::from_field((word & 3) as u8) as u8,
            "{word:#06x}"
        );
        assert_eq!(encoding::word1_fd(word), fpu_fd(word), "{word:#06x}");
        assert_eq!(
            encoding::word1_len(word),
            fpu_vector_len_field(word),
            "{word:#06x}"
        );
        assert_eq!(
            encoding::vector_subop(word),
            fpu_vector_subop_field(word),
            "{word:#06x}"
        );
        assert_eq!(
            encoding::scalar_subop(word),
            fpu_scalar_subop_field(word),
            "{word:#06x}"
        );
        assert_eq!(encoding::word1_mode(word), fpu_mode(word), "{word:#06x}");
    }
    for len in 0..4u8 {
        assert_eq!(
            encoding::decoded_last_lane(len),
            FpuVectorLength::from_field(len).map_or(3, |len| len.lanes() - 1),
            "len {len}"
        );
    }
}

/// Two-stage mirror of the shared CpuV3FpuMulPipe used by the leaf-level
/// SINCOS cycle test, since the pipeline itself lives in the unit top.
#[derive(Default)]
struct PipeStub {
    s1_valid: bool,
    s1_a: i64,
    s1_b: i64,
    s1_tag: u16,
    s2_valid: bool,
    s2_prod: i64,
    s2_tag: u16,
}

impl PipeStub {
    fn step(&mut self, abort: bool, in_valid: bool, in_a: i64, in_b: i64, in_tag: u16) {
        if abort {
            self.s1_valid = false;
            return;
        }
        self.s2_valid = self.s1_valid;
        if self.s1_valid {
            self.s2_prod = self.s1_a * self.s1_b;
            self.s2_tag = self.s1_tag;
        }
        self.s1_valid = in_valid;
        if in_valid {
            self.s1_a = in_a;
            self.s1_b = in_b;
            self.s1_tag = in_tag;
        }
    }

    fn out(&self) -> (bool, u64, u64) {
        (self.s2_valid, self.s2_prod as u64, u64::from(self.s2_tag))
    }
}

/// Drives the private special-path cycle model through one SINCOS the same
/// way the parent does (operand on the pre-T0 RF read, packed interval on
/// the one-cycle RF read) with the shared range return captured at T2 and
/// the first synchronous LUT read at T3. Check writes against
/// the fixed T6/T7 schedule and the frozen
/// `lut::sincos_q16` reference (itself the two-term C0/C1 reducer). This is
/// the leaf-level model/RTL agreement guard: the Verilog leaf testbench
/// checks the RTL against an independent reference, and this test pins the
/// Rust mirror and all three ISA modes to the same numbers.
#[test]
fn sincos_cycle_model_matches_reference() {
    let packed = lut::packed_sincos_lut();
    let fd = 8u16;
    let mut checked = 0u32;

    let mut check = |a: i32, mode: u16| {
        let word1 = (fd << 10) | (u16::from(encoding::SINCOS) << 4) | mode;
        let cycles = if matches!(mode, 1 | 2) { 7 } else { 8 };
        let mut state = CpuV3FpuSpecialPathState::default();
        let mut pipe = PipeStub::default();
        let mut read_a = a as u32;
        let mut writes: Vec<(u16, u32)> = Vec::new();
        let mut write_cycles = Vec::new();
        let mut product_cycles = Vec::new();
        let mut busy_count = 0u32;
        for cycle in 0..cycles + 2 {
            let (pipe_valid, pipe_product, pipe_tag) = pipe.out();
            if pipe_valid {
                product_cycles.push(cycle);
            }
            let input = CpuV3FpuSpecialPathInputValue {
                abort: false,
                instr_complete: cycle == 0,
                instr_opcode: u64::from(encoding::OPCODE_SCALAR),
                word1_raw: u64::from(word1),
                rf_read_a_data: u64::from(read_a),
                rf_read_b_data: 0,
                mul_out_valid: pipe_valid,
                mul_out_product: pipe_product,
                mul_out_tag: pipe_tag,
            };
            let out = state.comb(&input);
            if out.busy {
                busy_count += 1;
            }
            if out.rf_write_enable {
                writes.push((out.rf_write_address as u16, out.rf_write_data as u32));
                write_cycles.push(cycle);
            }
            pipe.step(
                false,
                out.mul_in_valid,
                out.mul_in_a as i64,
                out.mul_in_b as i64,
                out.mul_in_tag as u16,
            );
            state.tick(&input);
            // The synchronous RF presents the previous cycle's address.
            let address = out.rf_read_a_address as usize;
            read_a = if (256..512).contains(&address) {
                packed[address - 256]
            } else {
                0
            };
        }
        let (sin, cos) = lut::sincos_q16(a);
        let expected = match mode {
            1 => vec![(fd, sin as u32)],
            2 => vec![(fd, cos as u32)],
            _ => vec![(fd, sin as u32), (fd + 1, cos as u32)],
        };
        assert_eq!(
            writes, expected,
            "sincos cycle model mismatch at a={a}, mode={mode}"
        );
        assert_eq!(
            busy_count, cycles,
            "sincos busy length mismatch at a={a}, mode={mode}"
        );
        assert_eq!(
            product_cycles,
            if matches!(mode, 1 | 2) {
                vec![2, 6]
            } else {
                vec![2, 6, 7]
            },
            "range/interpolation shared returns"
        );
        let expected_write_cycles = if matches!(mode, 1 | 2) {
            vec![6]
        } else {
            vec![6, 7]
        };
        assert_eq!(write_cycles, expected_write_cycles, "SINCOS write schedule");
        checked += 1;
    };

    // [-2*pi, +2*pi] sampled densely, the exact quadrant endpoints, and
    // every power-of-two / full-range boundary.
    let limit = (2.0 * std::f64::consts::PI * 65536.0).round() as i32;
    let mut a = -limit;
    while a <= limit {
        check(a, 0);
        a += 37;
    }
    for a in [
        0,
        0x0001_0000,
        -0x0001_0000,
        102_944,  // pi/2
        -102_944, // -pi/2
        205_887,  // pi
        411_775,  // 2pi
        i32::MAX,
        i32::MIN,
    ] {
        check(a, 0);
        check(a, 1);
        check(a, 2);
    }
    let mut a = i32::MIN;
    while let Some(next) = a.checked_add(1_000_003) {
        check(a, 0);
        a = next;
    }
    assert!(checked > 1000, "expected a dense sweep, checked={checked}");
}

/// Abort through either final write edge gates the remaining results but
/// preserves an earlier published result. Immediate restart must use its
/// own phase, including abort on T2 capture or T3 synchronous lookup.
#[test]
fn sincos_cycle_model_abort_cancels_remaining_writes() {
    let packed = lut::packed_sincos_lut();
    let fd = 8u16;
    let a = 0x0001_0000i32;
    let word1 = (fd << 10) | (u16::from(encoding::SINCOS) << 4);
    for abort_cycle in 0..=7 {
        let mut state = CpuV3FpuSpecialPathState::default();
        let mut pipe = PipeStub::default();
        let mut read_a = a as u32;

        for cycle in 0..=abort_cycle {
            let abort = cycle == abort_cycle;
            let (pipe_valid, pipe_product, pipe_tag) = pipe.out();
            let input = CpuV3FpuSpecialPathInputValue {
                abort,
                instr_complete: cycle == 0,
                instr_opcode: u64::from(encoding::OPCODE_SCALAR),
                word1_raw: u64::from(word1),
                rf_read_a_data: u64::from(read_a),
                rf_read_b_data: 0,
                mul_out_valid: pipe_valid,
                mul_out_product: pipe_product,
                mul_out_tag: pipe_tag,
            };
            let out = state.comb(&input);
            if cycle < abort_cycle {
                assert!(out.busy, "SINCOS must be busy before abort");
            } else {
                assert!(!out.busy, "abort must clear busy at cycle {cycle}");
            }
            let published_before_abort = cycle == 6 && cycle < abort_cycle;
            assert_eq!(
                out.rf_write_enable, published_before_abort,
                "abort must preserve only earlier writes at cycle {cycle}"
            );
            if published_before_abort {
                assert_eq!(out.rf_write_address, u64::from(fd));
                assert_eq!(out.rf_write_data, u64::from(lut::sincos_q16(a).0 as u32));
            }
            pipe.step(
                abort,
                out.mul_in_valid,
                out.mul_in_a as i64,
                out.mul_in_b as i64,
                out.mul_in_tag as u16,
            );
            state.tick(&input);
            let address = out.rf_read_a_address as usize;
            read_a = if (256..512).contains(&address) {
                packed[address - 256]
            } else {
                0
            };
        }

        // Restart on the next edge with a distinct phase and destination.
        let next_a = -0x0002_0000i32 + abort_cycle;
        let next_fd = fd + 2;
        let word1 = (next_fd << 10) | (u16::from(encoding::SINCOS) << 4);
        let (sin, cos) = lut::sincos_q16(next_a);
        let mut read_a = next_a as u32;
        let mut got: Vec<(u16, u32)> = Vec::new();
        for cycle in 0..10 {
            let (pipe_valid, pipe_product, pipe_tag) = pipe.out();
            let input = CpuV3FpuSpecialPathInputValue {
                abort: false,
                instr_complete: cycle == 0,
                instr_opcode: u64::from(encoding::OPCODE_SCALAR),
                word1_raw: u64::from(word1),
                rf_read_a_data: u64::from(read_a),
                rf_read_b_data: 0,
                mul_out_valid: pipe_valid,
                mul_out_product: pipe_product,
                mul_out_tag: pipe_tag,
            };
            let out = state.comb(&input);
            if out.rf_write_enable {
                got.push((out.rf_write_address as u16, out.rf_write_data as u32));
            }
            pipe.step(
                false,
                out.mul_in_valid,
                out.mul_in_a as i64,
                out.mul_in_b as i64,
                out.mul_in_tag as u16,
            );
            state.tick(&input);
            let address = out.rf_read_a_address as usize;
            read_a = if (256..512).contains(&address) {
                packed[address - 256]
            } else {
                0
            };
        }
        assert_eq!(
            got,
            vec![(next_fd, sin as u32), (next_fd + 1, cos as u32)],
            "post-abort SINCOS must be clean at abort cycle {abort_cycle}"
        );
    }
}

/// Algebraic proof of the single-constant reducer: `C0*2^16 + C1 == K`, so
/// the 18-bit phase `(a * K) >>> 32` equals the low 18 bits of the frozen
/// full-width two-term reduction `h0 + ((((p0 + h1) << 16) + p1) >> 32)`
/// for every input. This isolates the exact identity the RTL now implements
/// through the shared 36x36 pipe and is independent of the LUT tables.
#[test]
fn sincos_k_reducer_matches_two_term_full_width() {
    const C0: i64 = 41_722;
    const C1: i64 = -31_890;

    fn full_width(a: i32) -> i32 {
        let a_hi = i64::from(a >> 16);
        let a_lo = i64::from(a & 0xFFFF);
        let p0 = a_lo * C0;
        let p1 = a_lo * C1;
        let h0 = a_hi * C0;
        let h1 = a_hi * C1;
        let t = h0 + ((((p0 + h1) << 16) + p1) >> 32);
        (t & 0x3_FFFF) as i32
    }

    fn k_reducer(a: i32) -> i32 {
        let product = i64::from(a) * SINCOS_K;
        ((product >> 32) & 0x3_FFFF) as i32
    }

    assert_eq!(C0 * (1 << 16) + C1, SINCOS_K, "K must be C0*2^16 + C1");

    let mut checked = 0u32;
    let mut verify = |a: i32| {
        assert_eq!(k_reducer(a), full_width(a), "K reducer mismatch at a={a}");
        checked += 1;
    };

    // Directed extremes and signed-boundary samples.
    for a in [
        0,
        1,
        -1,
        0x0000_FFFF,
        0x0001_0000,
        0x0001_0001,
        -0x0001_0000,
        -0x0001_0001,
        i32::MAX,
        i32::MIN,
        i32::MAX - 1,
        i32::MIN + 1,
    ] {
        verify(a);
    }

    // Dense [-2*pi, +2*pi].
    let limit = (2.0 * std::f64::consts::PI * 65536.0).round() as i32;
    let mut a = -limit;
    while a <= limit {
        verify(a);
        a += 37;
    }

    // Deterministic full-i32 sweep (same stride as the cycle-model test).
    let mut a = i32::MIN;
    while let Some(next) = a.checked_add(1_000_003) {
        verify(a);
        a = next;
    }
    assert!(checked > 1000, "expected a dense sweep, checked={checked}");
}
