//! Compiled RCC ownership/error controls. These peers do not certify scanout.
use cpu_v3::{rcc_backend, CpuV3Sim, RunOutcome};
use std::{any::Any, cell::RefCell, path::Path, rc::Rc};
#[derive(Default)]
struct State {
    active: u32,
    format: u16,
    pending: bool,
    next: u32,
    shadow: u32,
    frame: u16,
    error: u16,
    gpu: u16,
    starts: usize,
    swaps: usize,
    cache: bool,
    unstable: bool,
    reads: usize,
}
struct Peer {
    kind: u8,
    s: Rc<RefCell<State>>,
}
impl cpu_v3::Device for Peer {
    fn read(&mut self, _: &mut [u16], channel: u8) -> u16 {
        let mut s = self.s.borrow_mut();
        match (self.kind, channel) {
            (0, 5) => {
                if s.cache {
                    0x8000
                } else {
                    0
                }
            }
            (4, 2) => s.gpu,
            (3, 0) => s.frame,
            (3, 1) => s.active as u16,
            (3, 2) => (s.active >> 16) as u16,
            (3, 4) => s.format,
            (3, 3) => {
                s.reads += 1;
                s.error | u16::from(s.pending || (s.unstable && s.reads.is_multiple_of(2)))
            }
            _ => 0,
        }
    }
    fn write(&mut self, _: &mut [u16], channel: u8, value: u16) {
        let mut s = self.s.borrow_mut();
        match (self.kind, channel) {
            (4, 1) => {
                s.starts += 1;
                s.gpu = 1;
            }
            (3, 1) => s.shadow = (s.shadow & 0xffff0000) | u32::from(value),
            (3, 2) => s.shadow = (s.shadow & 65535) | (u32::from(value) << 16),
            (3, 4) => assert_eq!(value, 0),
            (3, 3) => {
                assert_eq!(value, 1);
                assert!(!s.pending);
                s.pending = true;
                s.next = s.shadow;
                s.swaps += 1;
            }
            (7, 0) => match value {
                0 => s.gpu = 0,
                1 => s.gpu = 1,
                2 => s.gpu = 2,
                3 => {
                    s.frame = s.frame.wrapping_add(1);
                    if s.pending {
                        s.active = s.next;
                        s.pending = false;
                    }
                }
                4 => s.frame = s.frame.wrapping_add(1),
                5 => s.error = 8,
                6 => s.error = 16,
                7 => s.error = 4,
                8 => s.error = 2,
                9 => s.active = 0x218000,
                10 => s.format = 1,
                11 => s.error = 0,
                12 => s.pending = true,
                13 => {
                    s.pending = false;
                    s.active = 0x218000;
                }
                14 => s.unstable = false,
                15 => s.cache = true,
                16 => s.cache = false,
                17 => s.frame = 65535,
                _ => panic!("stimulus"),
            },
            _ => {}
        }
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}
fn run(body: &str, initial: State) -> State {
    let source=format!("mod gpu;static N:u16=0;fn check(x:bool){{if !x{{halt(0xb00+N);}}unsafe{{addr_of(&N).write(0,N+1);}}}}fn main(){{{body}halt(0x600d);}}");
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("rcc");
    let options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    let parsed =
        rcc::frontend::compile_program_named("D3 controls", &source, &options, &mut |name| {
            std::fs::read_to_string(root.join(format!("{name}.rs"))).map_err(|e| e.to_string())
        })
        .unwrap();
    let program = rcc_backend::try_compile(parsed, &options, "main").unwrap();
    let state = Rc::new(RefCell::new(initial));
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&program.words);
    for kind in [0, 3, 4, 7] {
        cpu.attach_device(
            kind,
            Box::new(Peer {
                kind,
                s: state.clone(),
            }),
        );
    }
    let result = cpu.run(2_000_000).unwrap();
    assert!(
        matches!(result, RunOutcome::Halted { signal: 0x600d, .. }),
        "{result:?}"
    );
    drop(cpu);
    Rc::try_unwrap(state).ok().unwrap().into_inner()
}
fn initial() -> State {
    State {
        active: 0x200000,
        ..State::default()
    }
}
#[test]
fn compiled_cancel_discard_and_two_swaps_wait_for_real_active() {
    let s = run(
        r#"
 check(gpu_init()==GPU_IDLE);let c=gpu_cb_begin();check(c==1);check(gpu_cb_cancel(c));
 let c=gpu_cb_begin();let p=gpu_pass_begin(c,0,65535);check(p==1);check(GPU_CB.color==GPU_COLOR_B_SECTOR);check(gpu_cb_cancel(c));
 let c=gpu_cb_begin();check(gpu_pass_clear(c,0,65535));check(gpu_discard_ready(c));
 let c=gpu_cb_begin();check(gpu_pass_clear(c,0,65535));
 check(gpu_submit_idle(c)==GPU_PENDING);check(!gpu_cb_cancel(c));check(!gpu_discard_ready(c));check(gpu_wait(c,1)==GPU_TIMEOUT);
 dev_send(7,0,0);check(gpu_poll(c)==GPU_COMPLETED);check(gpu_cb_begin()==0);
 check(gpu_present(c)==GPU_PENDING);check(gpu_present(c)==GPU_REJECTED);check(gpu_poll(c)==GPU_IDLE);check(!gpu_discard_ready(c));check(gpu_cb_begin()==0);
 check(gpu_present_wait(0)==GPU_TIMEOUT);dev_send(7,0,17);dev_send(7,0,4);check(dev_recv(3,0)==0);
 check(gpu_present_wait(2)==GPU_TIMEOUT);check(GPU_DISPLAY.as_array()[1u16]==1&&GPU_DISPLAY.as_array()[2u16]==2);
 dev_send(7,0,4);check(gpu_present_poll()==GPU_PENDING);dev_send(7,0,3);check(gpu_present_wait(2)==GPU_COMPLETED);
 check(gpu_present_poll()==GPU_IDLE);check(GPU_DISPLAY.as_array()[1u16]==2);check(!gpu_discard_ready(c));
 let c=gpu_cb_begin();check(c==1);check(gpu_pass_clear(c,1,65535));check(GPU_CB.color==GPU_COLOR_A_SECTOR);
 check(gpu_submit_idle(c)==GPU_PENDING);dev_send(7,0,0);check(gpu_poll(c)==GPU_COMPLETED);
 check(gpu_present(c)==GPU_PENDING);dev_send(7,0,3);check(gpu_present_poll()==GPU_COMPLETED);check(GPU_DISPLAY.as_array()[1u16]==1);
 "#,
        initial(),
    );
    assert_eq!((s.starts, s.swaps), (2, 2));
}
#[test]
fn compiled_initialization_adopts_actual_front_and_holds_unknown_owners() {
    let s=run("check(gpu_init()==GPU_IDLE);let c=gpu_cb_begin();check(gpu_pass_clear(c,0,0));check(GPU_CB.color==GPU_COLOR_A_SECTOR);",State{active:0x218000,..initial()});
    assert_eq!(s.starts, 0);
    run(
        r#"check(gpu_init()==GPU_PENDING);check(gpu_cb_begin()==0);check(gpu_present_poll()==GPU_PENDING);
 dev_send(7,0,13);check(gpu_present_poll()==GPU_IDLE);check(gpu_cb_begin()==0);dev_send(7,0,0);check(gpu_poll(1)==GPU_IDLE);
 let c=gpu_cb_begin();check(gpu_pass_clear(c,0,0));check(GPU_CB.color==GPU_COLOR_A_SECTOR);"#,
        State {
            pending: true,
            gpu: 1,
            ..initial()
        },
    );
    run("check(gpu_init()==GPU_PENDING);check(gpu_cb_begin()==0);dev_send(7,0,14);check(gpu_present_poll()==GPU_IDLE);check(gpu_cb_begin()==1);",State{unstable:true,..initial()});
    for s in [
        State {
            active: 0,
            ..initial()
        },
        State {
            format: 1,
            ..initial()
        },
    ] {
        run("check(gpu_init()==GPU_FAULT);check(gpu_cb_begin()==0);check(gpu_init()==GPU_REJECTED);",s);
    }
}
#[test]
fn compiled_display_and_gpu_errors_never_release_pending() {
    for stimulus in [2, 5, 6, 7, 8, 9, 10] {
        let body = format!(
            r#"check(gpu_init()==GPU_IDLE);let c=gpu_cb_begin();check(gpu_pass_clear(c,0,0));check(gpu_submit_idle(c)==GPU_PENDING);
  dev_send(7,0,0);check(gpu_poll(c)==GPU_COMPLETED);check(gpu_present(c)==GPU_PENDING);dev_send(7,0,{stimulus});
  check(gpu_present_poll()==GPU_FAULT);check(GPU_DISPLAY.as_array()[1u16]==1&&GPU_DISPLAY.as_array()[2u16]==2);
  dev_send(7,0,11);dev_send(7,0,0);dev_send(7,0,3);check(gpu_present_poll()==GPU_FAULT);check(gpu_cb_begin()==0);check(!gpu_discard_ready(c));"#
        );
        let s = run(&body, initial());
        assert_eq!((s.starts, s.swaps), (1, 1));
    }
}
#[test]
fn compiled_invalid_target_and_maintenance_failure_do_not_launch() {
    let s = run(
        r#"check(gpu_init()==GPU_IDLE);let c=gpu_cb_begin();check(gpu_pass_clear(c,0,0));
 check(gpu_present(GPU_COMMAND_FIRST_SECTOR)==GPU_REJECTED);
 GPU_CB.color=GPU_COLOR_B_SECTOR+1;check(gpu_submit_idle(c)==GPU_REJECTED);GPU_CB.color=GPU_COLOR_B_SECTOR;
 dev_send(7,0,15);check(gpu_submit_idle(c)==GPU_CACHE_ERROR);dev_send(7,0,16);check(gpu_poll(c)==GPU_FAULT);check(gpu_present(c)==GPU_FAULT);
 check(gpu_cb_begin()==0);check(!gpu_discard_ready(c));"#,
        initial(),
    );
    assert_eq!((s.starts, s.swaps), (0, 0));
}
