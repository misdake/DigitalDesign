//! Bounded real-RCC reads and injected host events; no controller or RTL needed.
use cpu_v3::{rcc_backend, Device, RunOutcome, StepOutcome};
use cpu_v3_tang_nano_20k::{
    host_io::{map_xinput, ControllerSource, HostIo, XInputSample, XInputSource},
    input_device::{ControllerSnapshot, InputMode, InputSnapshotDevice},
    system_sim::CpuV3SystemSim,
};
use std::{cell::Cell, rc::Rc, time::Duration};

fn sample(buttons: u16) -> ControllerSnapshot {
    ControllerSnapshot {
        mode: InputMode::Analog,
        buttons_active_low: !buttons,
        right: [0, 255],
        left: [128, 64],
    }
}

fn observe_clock(system: &mut CpuV3SystemSim) -> (u16, u16) {
    let pc = system.cpu().pc();
    system
        .cpu_mut()
        .load_program(
            pc,
            &[
                cpu_v3::device_receive(2, 0, 10),
                cpu_v3::device_receive(3, 0, 11),
            ],
        )
        .unwrap();
    assert!(matches!(
        system.run(2).unwrap(),
        RunOutcome::StepLimit { .. }
    ));
    (
        system.cpu().register(2).unwrap(),
        system.cpu().register(3).unwrap(),
    )
}

#[test]
fn explicit_host_time_preserves_carry_wrap_clear_and_observation_purity() {
    let mut system = CpuV3SystemSim::default();
    assert_eq!(observe_clock(&mut system), (0, 0));
    system.advance_host_time(Duration::from_nanos(500));
    system.advance_host_time(Duration::from_nanos(500));
    assert_eq!(observe_clock(&mut system), (0, 1));
    system.advance_host_time(Duration::from_micros(999));
    assert_eq!(observe_clock(&mut system), (1, 1000));
    system.advance_vblank();
    assert_eq!(observe_clock(&mut system), (1, 1000));
    system.advance_host_time(Duration::from_millis(65535));
    assert_eq!(observe_clock(&mut system), (0, 65535));
    let pc = system.cpu().pc();
    let mut code = cpu_v3::load_immediate16(1, 1).to_vec();
    code.push(cpu_v3::device_send(1, 0, 11));
    system.cpu_mut().load_program(pc, &code).unwrap();
    system.run(code.len()).unwrap();
    assert_eq!(observe_clock(&mut system), (0, 0));
    system.advance_host_time(Duration::from_micros(23));
    assert_eq!(observe_clock(&mut system), (0, 23));
    system.advance_host_time(Duration::MAX);
    let expected = ((Duration::MAX.as_micros() + 23) / 1000 % 65536) as u16;
    assert_eq!(observe_clock(&mut system), (expected, 65535));
}

#[test]
fn snapshot_channel_order_sticky_publication_and_send_aliases() {
    let mut device = InputSnapshotDevice::default();
    assert_eq!(device.read(&mut [], 8), 0);
    device.publish(Some(sample(0x1234)));
    for (channel, value) in [
        (2, 0x73),
        (3, 0x5a),
        (4, 0xcb),
        (5, 0xed),
        (6, 0),
        (7, 255),
        (0, 128),
        (1, 64),
    ] {
        assert_eq!(device.read(&mut [], channel), value);
    }
    for channel in [8, 13, 14, 15] {
        assert_eq!(device.read(&mut [], channel), 3);
    }
    for channel in 0..8 {
        device.write(&mut [], channel, 0);
    }
    assert_eq!(device.read(&mut [], 8), 3);
    for channel in 8..16 {
        device.write(&mut [], channel, 65535);
        assert_eq!(device.read(&mut [], 8), 1);
        device.publish(Some(sample(1)));
        device.publish(Some(sample(2)));
        assert_eq!(device.read(&mut [], 8), 3);
        assert_eq!(device.read(&mut [], 4), 253);
    }
    device.write(&mut [], 8, 0);
    device.publish(None);
    assert_eq!(device.read(&mut [], 8), 2);
    device.write(&mut [], 8, 0);
    device.publish(None);
    assert_eq!(device.read(&mut [], 8), 0);
    for channel in 9..13 {
        assert_eq!(device.read(&mut [], channel), 0);
    }
}

#[test]
fn xinput_buttons_axes_and_independent_trigger_boundaries_match_the_abi() {
    for (button, expected) in [
        (0x20, 0),
        (0x40, 1),
        (0x80, 2),
        (0x10, 3),
        (1, 4),
        (8, 5),
        (2, 6),
        (4, 7),
        (0x100, 10),
        (0x200, 11),
        (0x8000, 12),
        (0x2000, 13),
        (0x1000, 14),
        (0x4000, 15),
    ] {
        let result = map_xinput(XInputSample {
            buttons: button,
            ..XInputSample::default()
        });
        assert_eq!(result.buttons_active_low, !(1 << expected));
        assert_eq!(result.left, [128; 2]);
        assert_eq!(result.right, [128; 2]);
    }
    for (trigger, expected) in [(0, 0), (30, 0), (31, 1), (255, 1)] {
        let result = map_xinput(XInputSample {
            buttons: 0x0c00,
            left_trigger: trigger,
            right_trigger: 0,
            left: [i16::MIN, i16::MAX],
            right: [i16::MAX, i16::MIN],
        });
        assert_eq!(result.buttons_active_low, !(expected << 8));
        assert_eq!(result.left, [0, 0]);
        assert_eq!(result.right, [255, 255]);
        let result = map_xinput(XInputSample {
            right_trigger: trigger,
            ..XInputSample::default()
        });
        assert_eq!(result.buttons_active_low, !(expected << 9));
    }
    assert!(XInputSource::new(4).is_err());
    #[cfg(not(windows))]
    assert!(XInputSource::new(0).is_err());
}

struct FakeInput(Rc<Cell<usize>>);
impl ControllerSource for FakeInput {
    fn poll(&mut self) -> Option<ControllerSnapshot> {
        let calls = self.0.get();
        self.0.set(calls + 1);
        (calls != 1).then_some(sample(1 << 14))
    }
}

#[test]
fn source_polling_focus_and_reconnect_use_host_events_without_vblank() {
    let calls = Rc::new(Cell::new(0));
    let mut host = HostIo::with_input(FakeInput(calls.clone()));
    let mut system = CpuV3SystemSim::default();
    host.advance(&mut system, Duration::from_millis(2), false);
    assert_eq!(calls.get(), 0);
    host.advance(&mut system, Duration::ZERO, true);
    assert_eq!(calls.get(), 1);
    host.advance(&mut system, Duration::from_millis(15), true);
    assert_eq!(calls.get(), 1);
    host.advance(&mut system, Duration::from_millis(1), true);
    assert_eq!(calls.get(), 2);
    host.advance(&mut system, Duration::from_millis(999), true);
    assert_eq!(calls.get(), 2);
    host.advance(&mut system, Duration::from_millis(1), true);
    assert_eq!(calls.get(), 3);
    host.advance(&mut system, Duration::ZERO, false);
    host.advance(&mut system, Duration::ZERO, true);
    assert_eq!(calls.get(), 4);
    assert_eq!(system.display_state().frame_index, 0);
    assert_eq!(observe_clock(&mut system), (1018, 65535));
}

fn compile(source: &str) -> Vec<u16> {
    let options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("rcc");
    let parsed = rcc::frontend::compile_program_named("host I/O", source, &options, &mut |name| {
        std::fs::read_to_string(root.join(format!("{name}.rs"))).map_err(|e| e.to_string())
    })
    .unwrap();
    rcc_backend::try_compile(parsed, &options, "main")
        .unwrap()
        .words
}

#[test]
fn real_rcc_timer_wait_progresses_only_after_host_elapsed_without_vblank() {
    let words = compile(
        r#"mod device_abi;mod time;
fn main(){let start=time_milliseconds();while time_elapsed_ms(start,time_milliseconds())<3{}
if time_microseconds()<3000{halt(0xbad);}halt(0x600d);}"#,
    );
    let mut system = CpuV3SystemSim::default();
    system.cpu_mut().set_boot_window(&words);
    for _ in 0..2 {
        assert!(matches!(
            system.run(1000).unwrap(),
            RunOutcome::StepLimit { .. }
        ));
    }
    let mut host = HostIo::default();
    host.advance(&mut system, Duration::from_millis(3), true);
    assert!(matches!(
        system.run(2000).unwrap(),
        RunOutcome::Halted { signal: 0x600d, .. }
    ));
    assert_eq!(system.display_state().frame_index, 0);
}

#[test]
fn host_time_releases_a_real_rcc_watch_without_retiring_waiting_instructions() {
    let words = compile(
        r#"mod device_abi;mod time;
fn main(){dev_send(0,6,80);dev_send(0,7,0);
if time_milliseconds()!=3{halt(0xbad);}halt(0x600d);}"#,
    );
    let mut system = CpuV3SystemSim::default();
    system.cpu_mut().set_boot_window(&words);
    assert!(matches!(
        system.run(500).unwrap(),
        RunOutcome::StepLimit { .. }
    ));
    let retired = system.cpu().retired_words();
    assert!(matches!(
        system.run(500).unwrap(),
        RunOutcome::StepLimit { .. }
    ));
    assert_eq!(system.cpu().retired_words(), retired);
    HostIo::default().advance(&mut system, Duration::from_millis(3), true);
    assert!(matches!(
        system.run(2000).unwrap(),
        RunOutcome::Halted { signal: 0x600d, .. }
    ));
    assert_eq!(system.display_state().frame_index, 0);
}

struct Phase(Rc<Cell<u16>>);
impl Device for Phase {
    fn read(&mut self, _: &mut [u16], _: u8) -> u16 {
        self.0.get()
    }
    fn write(&mut self, _: &mut [u16], _: u8, value: u16) {
        self.0.set(value);
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[test]
fn real_rcc_capture_preserves_edges_and_rejects_torn_latest_snapshots() {
    let words = compile(
        r#"mod dualshock2;
static S:Buf<u16,10>=Buf::new([0;10]);static RX:Buf<u16,9>=Buf::new([0;9]);
fn check(ok:bool){if !ok{halt(0xbad);}}
fn capture()->bool{ds2_capture(S.as_array(),RX.as_array())}
fn main(){ds2_init(S.as_array());check(!capture());
dev_send(7,0,1);check(capture()&&S[0u16]==1&&S[8u16]==0&&S[3u16]==0&&S[4u16]==255);
dev_send(7,0,2);check(capture()&&S[2u16]==16384&&S[8u16]==16384);
check(!capture()&&S[8u16]==16384);ds2_consume_edges(S.as_array());
dev_send(7,0,3);check(!capture()&&S[2u16]==16384&&S[8u16]==0&&S[9u16]==0);
check(capture()&&S[2u16]==8192&&S[8u16]==8192&&S[9u16]==16384);
dev_send(7,0,4);check(capture()&&S[1u16]==65&&S[3u16]==128&&S[4u16]==128&&S[5u16]==128&&S[6u16]==128);
dev_send(7,0,5);check(!capture()&&S[0u16]==0&&S[2u16]==0&&S[8u16]==0&&S[9u16]==0);
dev_send(7,0,6);check(capture()&&S[2u16]==4096&&S[8u16]==0&&S[9u16]==0);halt(0x600d);}"#,
    );
    let mut system = CpuV3SystemSim::default();
    system.cpu_mut().set_boot_window(&words);
    let phase = Rc::new(Cell::new(0));
    system
        .cpu_mut()
        .attach_device(7, Box::new(Phase(phase.clone())));
    let mut previous = 0;
    let mut torn = false;
    for _ in 0..100_000 {
        let current = phase.get();
        if current != previous {
            match current {
                1 => system.publish_input(Some(sample(0))),
                2 => system.publish_input(Some(sample(1 << 14))),
                3 => system.publish_input(Some(sample((1 << 14) | (1 << 12)))),
                4 => system.publish_input(Some(ControllerSnapshot {
                    mode: InputMode::Digital,
                    ..sample(0)
                })),
                5 => system.publish_input(None),
                6 => {
                    for buttons in [1, 2, 1 << 12] {
                        system.publish_input(Some(sample(buttons)));
                    }
                }
                _ => panic!("unexpected phase"),
            }
            previous = current;
        }
        let pc = usize::from(system.cpu().pc());
        if current == 3 && !torn && words.get(pc).is_some_and(|word| word & 0xfff0 == 0x7550) {
            system.publish_input(Some(sample(1 << 13)));
            torn = true;
        }
        if let StepOutcome::Halted { signal } = system.step().unwrap() {
            assert_eq!(signal, 0x600d);
            assert!(torn);
            return;
        }
    }
    panic!("bounded RCC snapshot run did not finish");
}

#[cfg(windows)]
#[test]
#[ignore = "calls the real Windows API; no controller required"]
fn windows_xinput_get_state_smoke() {
    use windows_sys::Win32::UI::Input::XboxController::{XInputGetState, XINPUT_STATE};
    let mut state: XINPUT_STATE = unsafe { std::mem::zeroed() };
    let code = unsafe { XInputGetState(0, &mut state) };
    assert!(code == 0 || code == 1167, "unexpected XInput status {code}");
    println!("real XInputGetState(0) status={code}; no physical input mapping claim");
}
