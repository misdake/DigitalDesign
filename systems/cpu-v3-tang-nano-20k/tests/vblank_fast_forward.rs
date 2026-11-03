//! Bounded event equivalence, real RCC waits and native-cycle fallback.
use cpu_v3::{device_receive, halt, rcc_backend, Device, StepOutcome};
use cpu_v3_tang_nano_20k::{
    boot::{self, SystemControlDevice},
    input_device::{ControllerSnapshot, InputMode},
    system_emu::CpuV3SystemEmu,
    system_sim::{CpuV3SystemSim, DisplayState, VBlankMode},
    DisplayDevice, ObservationDevice, DISPLAY_CONTROL, DISPLAY_DEVICE, DISPLAY_FRAMEBUFFER_HIGH,
    DISPLAY_FRAMEBUFFER_LOW, DISPLAY_FRAME_INDEX, DISPLAY_NEXT_SWAP,
};
use std::{
    path::Path,
    sync::OnceLock,
    time::{Duration, Instant},
};

fn package() -> &'static [u8] {
    static IMAGE: OnceLock<Vec<u8>> = OnceLock::new();
    IMAGE.get_or_init(|| {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("rcc");
        let options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
        let source = r#"mod gpu_present;
fn main(){
 if !gpu_present_open() { halt(0xbad1); }
 if !gpu_present_set_next(0x8800) { halt(0xbad2); }
 if !gpu_present_wait_vblank() { halt(0xbad3); }
 if !gpu_present_wait_vblank() { halt(0xbad4); }
 dev_send(0,3,90);
 dev_send(7,0,dev_recv(0,10));
 dev_send(7,1,dev_recv(0,11));
 dev_send(7,2,dev_recv(5,8));
 halt(0x600d);
}"#;
        let check = rcc::frontend::rustc_check::RustcCheck::new(
            root.join("../../../target/vblank/rustc-check"),
        );
        let parsed = rcc::frontend::compile_program_named_checked(
            "vblank-fast-forward",
            source,
            &options,
            &check,
            &mut |name| {
                std::fs::read_to_string(root.join(format!("{name}.rs"))).map_err(|e| e.to_string())
            },
        )
        .unwrap();
        let static_words = parsed.debug.static_data_end;
        let program = rcc_backend::try_compile(parsed, &options, "main").unwrap();
        let bytes: Vec<_> = program.words.iter().flat_map(|w| w.to_le_bytes()).collect();
        boot::pack_single_application(&bytes, static_words).unwrap()
    })
}

fn arm(sim: &mut CpuV3SystemSim, device: u8, channel: u8, expected: u16) {
    let mut control = SystemControlDevice::default();
    control.write(
        &mut [],
        boot::SYSCTL_WATCH_TARGET,
        boot::sysctl_watch_target(device, channel),
    );
    control.write(&mut [], boot::SYSCTL_WATCH_EXPECTED, expected);
    sim.cpu_mut().attach_device(0, Box::new(control));
}

fn pending_display() -> DisplayDevice {
    let mut d = DisplayDevice::default();
    d.write(&mut [], DISPLAY_FRAMEBUFFER_LOW, 0);
    d.write(&mut [], DISPLAY_FRAMEBUFFER_HIGH, 0x22);
    d.write(&mut [], DISPLAY_CONTROL, DISPLAY_NEXT_SWAP);
    d
}

#[test]
fn only_an_equal_display_watch_can_yield() {
    let mut sim = CpuV3SystemSim::new(VBlankMode::PauseOnFrameIndexWait);
    sim.cpu_mut()
        .attach_device(DISPLAY_DEVICE, Box::new(pending_display()));
    assert!(sim.waiting_for_vblank());
    assert!(
        !sim.blocked_on_vblank(),
        "a queued swap does not block CPU execution"
    );
    for (device, channel, expected, blocked) in [
        (3, DISPLAY_CONTROL, 1, true),
        (3, DISPLAY_CONTROL, 0, false),
        (3, DISPLAY_FRAME_INDEX, 0, true),
        (3, DISPLAY_FRAME_INDEX, 1, false),
        (3, DISPLAY_FRAMEBUFFER_LOW, 0, false),
        (0, boot::SYSCTL_MILLISECONDS, 0, false),
        (4, 0, 0, false),
        (5, 8, 0, false),
    ] {
        arm(&mut sim, device, channel, expected);
        assert_eq!(
            sim.blocked_on_vblank(),
            blocked,
            "{device}/{channel}/{expected}"
        );
    }
    let mut invalid = pending_display();
    invalid.write(&mut [], DISPLAY_FRAMEBUFFER_LOW, 1);
    invalid.write(&mut [], DISPLAY_FRAMEBUFFER_HIGH, 0x22);
    invalid.write(&mut [], DISPLAY_CONTROL, DISPLAY_NEXT_SWAP);
    sim.cpu_mut().attach_device(3, Box::new(invalid));
    arm(&mut sim, 3, DISPLAY_CONTROL, 1);
    assert!(
        !sim.blocked_on_vblank(),
        "a changed/error status must release, not yield"
    );
    let mut reads = CpuV3SystemSim::new(VBlankMode::PauseOnFrameIndexWait);
    reads
        .cpu_mut()
        .load_program(
            0,
            &[device_receive(0, 3, 0), device_receive(0, 3, 0), halt()],
        )
        .unwrap();
    reads.step().unwrap();
    reads.step().unwrap();
    assert!(reads.waiting_for_vblank());
    assert!(
        !reads.blocked_on_vblank(),
        "two ordinary reads are not a sleep proof"
    );
}

#[test]
fn changed_value_and_frame_wrap_release_without_an_extra_vblank() {
    let mut sim = CpuV3SystemSim::default();
    sim.cpu_mut().load_program(0, &[halt()]).unwrap();
    for _ in 0..65535 {
        sim.advance_vblank();
    }
    arm(&mut sim, 3, DISPLAY_FRAME_INDEX, 65535);
    assert!(sim.blocked_on_vblank());
    sim.advance_vblank();
    assert_eq!(sim.display_state().frame_index, 0);
    assert!(!sim.blocked_on_vblank());
    sim.step().unwrap();
    assert_eq!(
        sim.cpu().retired_words(),
        0,
        "the release observation is still a real step"
    );
    assert!(matches!(sim.step().unwrap(), StepOutcome::Halted { .. }));
}

#[derive(Debug, PartialEq, Eq)]
struct Snapshot {
    registers: Vec<u16>,
    memory: Vec<u16>,
    retired: u64,
    display: DisplayState,
    uart: Vec<u8>,
    observed: [Option<u16>; 3],
}

fn drive(fast: bool) -> (Snapshot, usize, Duration) {
    let mut sim = CpuV3SystemSim::from_single_flash(package(), 0xa55a, VBlankMode::Manual).unwrap();
    let mut calls = 0;
    let mut halted = false;
    let begin = Instant::now();
    for sample in 0..3 {
        sim.advance_host_time(Duration::from_micros(16667));
        sim.publish_input(Some(ControllerSnapshot {
            mode: InputMode::Analog,
            buttons_active_low: 0xffff ^ (1 << sample),
            left: [128; 2],
            right: [128; 2],
        }));
        for _ in 0..30000 {
            if fast && sim.blocked_on_vblank() {
                break;
            }
            calls += 1;
            if let StepOutcome::Halted { signal } = sim.step().unwrap() {
                assert_eq!(signal, 0x600d);
                halted = true;
                break;
            }
        }
        if halted {
            break;
        }
        sim.advance_vblank();
    }
    assert!(halted, "bounded two-vblank RCC fixture did not halt");
    let elapsed = begin.elapsed();
    let cpu = sim.cpu();
    let markers = cpu.device::<ObservationDevice>(7).unwrap();
    (
        Snapshot {
            registers: (0..16).map(|i| cpu.register(i).unwrap()).collect(),
            memory: cpu.physical_memory_slice().to_vec(),
            retired: cpu.retired_words(),
            display: sim.display_state(),
            uart: cpu.device::<SystemControlDevice>(0).unwrap().uart.clone(),
            observed: [markers.last(0), markers.last(1), markers.last(2)],
        },
        calls,
        elapsed,
    )
}

#[test]
fn real_rcc_waits_match_unskipped_memory_time_input_and_uart() {
    let (reference, slow_calls, slow_time) = drive(false);
    let (fast, fast_calls, fast_time) = drive(true);
    assert_eq!(fast, reference);
    assert_eq!(fast.observed[0], Some(50));
    assert_eq!(fast.observed[1], Some(50001));
    assert_eq!(fast.uart, b"Z");
    assert!(slow_calls > fast_calls * 10);
    eprintln!("wait-only host comparison: reference={slow_calls} calls/{slow_time:?}, fast={fast_calls} calls/{fast_time:?}; identical two-vblank work, no physical clock conversion");
}

#[test]
fn real_rcc_read_to_arm_races_do_not_lose_the_event() {
    for channel in [DISPLAY_CONTROL, DISPLAY_FRAME_INDEX] {
        let mut sim =
            CpuV3SystemSim::from_single_flash(package(), 0xa55a, VBlankMode::Manual).unwrap();
        let mut selector = 0;
        let mut injected = false;
        let mut halted = false;
        for _ in 0..20000 {
            if sim.blocked_on_vblank() {
                sim.advance_vblank();
            }
            let step = sim
                .step_observing(|write, _| {
                    if write.device == 0 && write.channel == boot::SYSCTL_WATCH_TARGET {
                        selector = write.value;
                    }
                    !(write.device == 0
                        && write.channel == boot::SYSCTL_WATCH_EXPECTED
                        && selector == boot::sysctl_watch_target(3, channel)
                        && !injected)
                })
                .unwrap();
            if step.is_none() {
                injected = true;
                sim.advance_vblank();
                sim.step().unwrap();
                assert!(
                    !sim.blocked_on_vblank(),
                    "the event occurred before the arm committed"
                );
            } else if let Some(StepOutcome::Halted { signal }) = step {
                assert_eq!(signal, 0x600d);
                halted = true;
                break;
            }
        }
        assert!(injected && halted, "bounded read-to-arm race/{channel}");
    }
}

#[test]
fn a_swap_completed_before_the_first_poll_does_not_arm_a_pending_watch() {
    let mut sim = CpuV3SystemSim::from_single_flash(package(), 0xa55a, VBlankMode::Manual).unwrap();
    let mut applied = false;
    let mut control_watches = 0;
    let mut halted = false;
    for _ in 0..20000 {
        if sim.blocked_on_vblank() {
            sim.advance_vblank();
        }
        let step = sim
            .step_observing(|write, _| {
                if write.device == 0
                    && write.channel == boot::SYSCTL_WATCH_TARGET
                    && write.value == boot::sysctl_watch_target(3, DISPLAY_CONTROL)
                {
                    control_watches += 1;
                }
                !(write.device == 3 && write.channel == DISPLAY_CONTROL && !applied)
            })
            .unwrap();
        if step.is_none() {
            applied = true;
            sim.step().unwrap();
            assert!(sim.advance_vblank());
        } else if let Some(StepOutcome::Halted { signal }) = step {
            assert_eq!(signal, 0x600d);
            halted = true;
            break;
        }
    }
    assert!(applied && halted);
    assert_eq!(control_watches, 0);
}

#[test]
fn actual_cli_yields_within_budgets_and_has_an_unskipped_reference() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/vblank/cli");
    std::fs::create_dir_all(&root).unwrap();
    let image = root.join("wait.cb06");
    std::fs::write(&image, package()).unwrap();
    let mut summaries = Vec::new();
    for fast in [false, true] {
        let out = root.join(if fast { "fast" } else { "reference" });
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_cpu-v3-display-sim"));
        command
            .args(["--package"])
            .arg(&image)
            .args(["--max-cpu-steps", "90000", "--frames", "3", "--out-dir"])
            .arg(&out);
        if !fast {
            command.arg("--no-vblank-fast-forward");
        }
        let result = command.output().unwrap();
        std::fs::write(out.join("stdout.log"), &result.stdout).unwrap();
        std::fs::write(out.join("stderr.log"), &result.stderr).unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let summary: serde_json::Value =
            serde_json::from_slice(&std::fs::read(out.join("summary-instruction.json")).unwrap())
                .unwrap();
        assert_eq!(summary["stop_reason"], "halt");
        assert_eq!(summary["presented_swaps"], 1);
        assert_eq!(summary["front_valid"], true);
        assert_eq!(summary["gpu_error"], false);
        assert!(summary["logic_cycles"].is_null());
        assert_eq!(summary["vblank_fast_forward"], fast);
        assert_eq!(summary["vblank_yields"], if fast { 2 } else { 0 });
        summaries.push(summary);
    }
    assert!(
        summaries[0]["cpu_step_calls"].as_u64().unwrap()
            > summaries[1]["cpu_step_calls"].as_u64().unwrap() * 10
    );
    for (label, steps, frames, expected) in [
        ("one-step", "1", "3", "step-limit"),
        ("one-sample", "90000", "1", "sample-limit"),
    ] {
        let out = root.join(label);
        let status = std::process::Command::new(env!("CARGO_BIN_EXE_cpu-v3-display-sim"))
            .arg("--package")
            .arg(&image)
            .args(["--max-cpu-steps", steps, "--frames", frames, "--out-dir"])
            .arg(&out)
            .output()
            .unwrap();
        assert!(status.status.success());
        let summary: serde_json::Value =
            serde_json::from_slice(&std::fs::read(out.join("summary-instruction.json")).unwrap())
                .unwrap();
        assert_eq!(summary["stop_reason"], expected);
        assert!(summary["cpu_step_calls"].as_u64().unwrap() <= steps.parse::<u64>().unwrap());
        assert!(summary["samples"].as_u64().unwrap() <= frames.parse::<u64>().unwrap());
    }
}

#[test]
fn native_waits_keep_real_memory_and_timer_edges_running() {
    let mut sim = CpuV3SystemEmu::from_single_flash(package(), 0xa55a, VBlankMode::Manual).unwrap();
    let mut sleeps = 0;
    let mut halted = false;
    for _ in 0..60000 {
        if sim.blocked_on_vblank() {
            let clocks = (sim.cycles(), sim.native_cycles());
            let retired = sim.core().retired_words();
            for _ in 0..100 {
                sim.step().unwrap();
            }
            assert_eq!(sim.cycles(), clocks.0 + 100);
            assert_eq!(sim.native_cycles(), clocks.1 + 200);
            assert_eq!(sim.core().retired_words(), retired);
            assert!(sim.blocked_on_vblank());
            sim.advance_vblank();
            sleeps += 1;
        }
        let step = sim.step().unwrap();
        assert!(!step.fault, "native fault {}", step.fault_code);
        if step.halted {
            assert_eq!(step.halt_signal, 0x600d);
            halted = true;
            break;
        }
    }
    assert!(halted && sleeps == 2, "bounded native fallback/{sleeps}");
}
