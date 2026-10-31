//! Display-only protocol tests, with delayed old-read retirement after vblank.
use cpu_v3::{rcc_backend, CpuV3Sim, Device, StepOutcome};
use rcc::frontend::{compile_program_named_checked, rustc_check::RustcCheck};
use std::{any::Any, cell::RefCell, path::Path, rc::Rc};

pub(super) struct State {
    active: u32,
    shadow: u32,
    format: u16,
    frame: u16,
    pending: u16,
    remaining_reads: u16,
    wrong_target: bool,
    pub(super) events: Vec<(u8, u16)>,
}
pub(super) struct Peer {
    pub(super) kind: u8,
    pub(super) state: Rc<RefCell<State>>,
}
pub(super) fn peer_state(wrong_target: bool) -> Rc<RefCell<State>> {
    Rc::new(RefCell::new(State {
        active: 0x8000 * 64,
        shadow: 0,
        format: 1,
        frame: 65535,
        pending: 0,
        remaining_reads: 0,
        wrong_target,
        events: Vec::new(),
    }))
}
impl Device for Peer {
    fn read(&mut self, _: &mut [u16], channel: u8) -> u16 {
        assert_eq!(self.kind, 3, "Display must not query/wait GPU");
        let mut s = self.state.borrow_mut();
        match channel {
            0 => {
                let frame = s.frame;
                if s.pending == 0 {
                    s.frame = s.frame.wrapping_add(1);
                }
                frame
            }
            1 => s.active as u16,
            2 => (s.active >> 16) as u16,
            3 => {
                if s.pending != 0 {
                    if s.remaining_reads == 8 {
                        s.frame = s.frame.wrapping_add(1);
                    }
                    s.remaining_reads -= 1;
                    if s.remaining_reads == 0 {
                        s.pending = 0;
                        s.active = if s.wrong_target {
                            0x8000 * 64
                        } else {
                            s.shadow
                        };
                        s.format = 0;
                    }
                }
                s.pending
            }
            4 => s.format,
            _ => panic!("unexpected Display channel {channel}"),
        }
    }
    fn write(&mut self, _: &mut [u16], channel: u8, value: u16) {
        assert_eq!(self.kind, 3, "Display must not send GPU commands");
        let mut s = self.state.borrow_mut();
        s.events.push((channel, value));
        match channel {
            1 => s.shadow = (s.shadow & 0xffff0000) | u32::from(value),
            2 => s.shadow = (s.shadow & 65535) | (u32::from(value) << 16),
            4 => assert_eq!(value, 0),
            3 => {
                assert_eq!(value, 1);
                assert_eq!(s.pending, 0);
                s.pending = 1;
                s.remaining_reads = 8;
            }
            _ => panic!("unexpected Display write {channel}"),
        }
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}
fn run(label: &str, body: &str, wrong_target: bool) -> Vec<(u8, u16)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("rcc");
    let out = std::env::var_os("RCC_RUSTC_TEST_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("../../../target/graphics-api-g0"))
        .join(format!("present-{label}"));
    let source = format!(
        r#"mod gpu_present;
static STEP:u16=0;
fn check(ok:bool){{if !ok{{halt(0xd000+STEP);}}unsafe{{addr_of(&STEP).write(0,STEP+1);}}}}
fn main(){{check(gpu_present_open());{body}halt(0x600d);}}"#
    );
    let options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    let parsed = compile_program_named_checked(
        "api-present-peer",
        &source,
        &options,
        &RustcCheck::new(out),
        &mut |name| {
            std::fs::read_to_string(root.join(format!("{name}.rs"))).map_err(|e| e.to_string())
        },
    )
    .unwrap();
    let program = rcc_backend::try_compile(parsed, &options, "main").unwrap();
    let shared = peer_state(wrong_target);
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&program.words);
    for kind in [3, 4] {
        cpu.attach_device(
            kind,
            Box::new(Peer {
                kind,
                state: shared.clone(),
            }),
        );
    }
    let mut halted = false;
    for _ in 0..500_000 {
        if let StepOutcome::Halted { signal } = cpu.step().unwrap() {
            assert_eq!(
                signal,
                0x600d,
                "{label}, check {}",
                signal.wrapping_sub(0xd000)
            );
            halted = true;
            break;
        }
    }
    assert!(halted, "bounded Display peer timeout: {label}");
    let result = shared.borrow().events.clone();
    result
}
#[test]
fn display_waits_for_actual_swap_and_old_reads_not_frame_change_alone() {
    let events = run(
        "delayed-retirement",
        r#"
 let before=dev_recv(DISPLAY_DEVICE,DISPLAY_FRAME_INDEX);
 check(gpu_present_set_next(0x8800));check(gpu_present_poll()==1);
 check(dev_recv(DISPLAY_DEVICE,DISPLAY_FRAME_INDEX)!=before);check(GPU_PRESENT[1u16]==1);
 check(gpu_present_poll()==1);check(gpu_present_wait_vblank());
 check(GPU_PRESENT[1u16]==0);check(gpu_present_poll()==0);
 check(gpu_present_wait_vblank());
 "#,
        false,
    );
    assert_eq!(events, vec![(1, 0), (2, 0x22), (4, 0), (3, 1)]);
}
#[test]
fn display_faults_do_not_drop_pending_or_emit_a_second_request() {
    let duplicate = run(
        "duplicate",
        r#"
 check(gpu_present_set_next(0x8800));check(!gpu_present_set_next(0x9000));
 check(GPU_PRESENT[1u16]==1&&gpu_present_poll()==65535);
 "#,
        false,
    );
    assert_eq!(duplicate.len(), 4);
    let wrong = run(
        "wrong-target",
        r#"
 check(gpu_present_set_next(0x8800));check(!gpu_present_wait_vblank());
 check(GPU_PRESENT[1u16]==1&&GPU_PRESENT[3u16]==4);
 "#,
        true,
    );
    assert_eq!(wrong.len(), 4);
    let invalid = run(
        "unaligned",
        r#"check(!gpu_present_set_next(0x8801));"#,
        false,
    );
    assert!(invalid.is_empty());
}
