use super::*;

/// The release v3 geometry library compiles to existing FPU v2 instructions.
/// This runs each generated sequence on the architectural simulator and checks
/// the result against the reference model, so it pins the lowering without
/// needing Icarus and keeps every program under a bounded step count.
#[test]
fn emulator_matches_reference_for_compiled_geometry_helpers() {
    for &(ax, ay, bx, by, threshold) in GEOMETRY_CASES {
        let program = compile(&geometry_helper_source(ax, ay, bx, by, threshold));
        let expected = geometry_expected_signal(
            i32::from(ax),
            i32::from(ay),
            i32::from(bx),
            i32::from(by),
            i32::from(threshold),
        );
        let signal = run_program_signal(&program, 20_000);
        assert_eq!(
            signal, expected,
            "geometry helper mismatch for a=({ax},{ay}) b=({bx},{by}) threshold={threshold}"
        );
    }
}

/// The `_checked` helpers must return exactly the release results whenever the
/// small-range contract holds.
#[test]
fn checked_geometry_helpers_match_unchecked_on_valid_input() {
    for &(ax, ay, bx, by, threshold) in GEOMETRY_CASES {
        let unchecked = compile(&geometry_helper_source(ax, ay, bx, by, threshold));
        let checked = compile(&geometry_checked_source(ax, ay, bx, by, threshold));
        let unchecked_signal = run_program_signal(&unchecked, 20_000);
        let checked_signal = run_program_signal(&checked, 20_000);
        assert_eq!(
            checked_signal, unchecked_signal,
            "checked helper diverged for a=({ax},{ay}) b=({bx},{by}) threshold={threshold}"
        );
    }
}

/// `v3_length2_checked` halts with its documented signal on a bad component.
#[test]
fn checked_length2_halts_with_its_signal() {
    let program = compile(
        "fn main() { let a = vec3::new(fix32::from_int(105), fix32::zero(), fix32::zero()); \
         let s = v3_length2_checked(a); halt(s.lo_bits()); }",
    );
    assert_eq!(
        run_program_signal(&program, 20_000),
        cpu_v3::V3_LENGTH2_CHECKED_HALT
    );
}

/// `v3_normalize_checked` halts with its documented signal on a bad component.
#[test]
fn checked_normalize_halts_with_its_signal() {
    let program = compile(
        "fn main() { let a = vec3::new(fix32::from_int(105), fix32::zero(), fix32::zero()); \
         let n = v3_normalize_checked(a); halt(n.x().lo_bits()); }",
    );
    assert_eq!(
        run_program_signal(&program, 20_000),
        cpu_v3::V3_NORMALIZE_CHECKED_HALT
    );
}

/// `v3_distance_gt_checked` halts with the input-range signal when a raw input
/// component cannot be safely subtracted.
#[test]
fn checked_distance_halts_with_the_input_signal() {
    let program = compile(
        "fn main() { let a = vec3::new(fix32::from_int(16384), fix32::zero(), fix32::zero()); \
         let b = vec3::zero(); let g = v3_distance_gt_checked(a, b, fix32::zero()); halt(g as u16); }",
    );
    assert_eq!(
        run_program_signal(&program, 20_000),
        cpu_v3::V3_DISTANCE_GT_CHECKED_INPUT_HALT
    );
}

/// `v3_distance_gt_checked` halts with the difference-range signal when the
/// in-range inputs differ by more than the `DOTSTORE` can hold.
#[test]
fn checked_distance_halts_with_the_difference_signal() {
    let program = compile(
        "fn main() { let a = vec3::new(fix32::from_int(100), fix32::zero(), fix32::zero()); \
         let b = vec3::new(fix32::from_int(-100), fix32::zero(), fix32::zero()); \
         let g = v3_distance_gt_checked(a, b, fix32::zero()); halt(g as u16); }",
    );
    assert_eq!(
        run_program_signal(&program, 20_000),
        cpu_v3::V3_DISTANCE_GT_CHECKED_DIFFERENCE_HALT
    );
}

#[test]
#[ignore = "explicit emulator-vs-Icarus co-simulation of a compiled geometry-library FPU program"]
fn core_emu_matches_rtl_compiled_geometry_fpu_program() {
    let (ax, ay, bx, by, threshold) = GEOMETRY_CASES[0];
    let program = compile(&geometry_helper_source(ax, ay, bx, by, threshold));
    let expected = geometry_expected_signal(
        i32::from(ax),
        i32::from(ay),
        i32::from(bx),
        i32::from(by),
        i32::from(threshold),
    );
    let module_name = CpuV3Core::verilog_identity().module_name();
    let emu = run_core_emu_trace(&program, 20_000);
    assert!(!emu.is_empty(), "emu trace empty");
    let last_emu = emu.last().copied().expect("emu trace non-empty");
    assert!(
        !last_emu.fault,
        "compiled geometry program faulted in the cycle model: code={} pc={:#06x}",
        last_emu.fault_code, last_emu.fault_pc
    );
    assert!(last_emu.halted, "compiled geometry program did not halt");
    assert_eq!(last_emu.halt_signal, expected, "unexpected geometry result");

    let max_cycles = emu.len() + 800;
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
