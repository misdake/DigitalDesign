//! Offline execution-protocol peer; this never renders or accesses a board.
use cpu_v3::{rcc_backend, CpuV3Sim, Device, StepOutcome};
use rcc::frontend::{compile_program_named_checked, rustc_check::RustcCheck};
use std::{any::Any, cell::RefCell, collections::VecDeque, path::Path, rc::Rc};

const SETUP: &str = r#"
mod gpu_execute;
static CODE:Buf<u16,2>=Buf::new([3,0x4000]);
static STEP:u16=0;
fn check(ok:bool){if !ok{halt(0xd000+STEP);}unsafe{addr_of(&STEP).write(0,STEP+1);}}
fn make(slot:u16){
 let command:u16=if slot==0{0x9800}else{0xfc00};let push:u16=if slot==0{0x9a00}else{0xfe00};
 check(gpu_record_configure(slot,command,1,push,1));check(gpu_record_begin_pass(slot));
 check(gpu_record_append(slot,CODE.as_array(),2,CODE.as_array(),0,1));check(gpu_record_close_pass(slot));
}
"#;

#[derive(Default)]
struct State {
    statuses: VecDeque<u16>,
    status_reads: usize,
    cache_error: bool,
    events: Vec<(u8, u8, u16)>,
}
struct Peer {
    kind: u8,
    state: Rc<RefCell<State>>,
}
impl Device for Peer {
    fn read(&mut self, _: &mut [u16], channel: u8) -> u16 {
        let mut s = self.state.borrow_mut();
        match (self.kind, channel) {
            (4, 2) => {
                s.status_reads += 1;
                s.statuses.pop_front().unwrap_or(0)
            }
            (0, 5) if s.cache_error => 0x8000,
            _ => 0,
        }
    }
    fn write(&mut self, _: &mut [u16], channel: u8, value: u16) {
        if (self.kind == 0 && channel == 1) || self.kind == 4 {
            self.state
                .borrow_mut()
                .events
                .push((self.kind, channel, value));
        }
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

fn run(label: &str, body: &str, statuses: &[u16], cache_error: bool) -> Vec<(u8, u8, u16)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("rcc");
    let out = std::env::var_os("RCC_RUSTC_TEST_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("../../../target/graphics-api-g0"))
        .join(format!("execution-{label}"));
    let options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    let source = format!("{SETUP}\nfn main(){{gpu_record_init();check(gpu_execute_init());make(0);make(1);{body}\nhalt(0x600d);}}");
    let parsed = compile_program_named_checked(
        "graphics-api-execution-peer",
        &source,
        &options,
        &RustcCheck::new(out),
        &mut |name| {
            std::fs::read_to_string(root.join(format!("{name}.rs"))).map_err(|e| e.to_string())
        },
    )
    .unwrap();
    let program = rcc_backend::try_compile(parsed, &options, "main").unwrap();
    let shared = Rc::new(RefCell::new(State {
        statuses: statuses.iter().copied().collect(),
        cache_error,
        ..State::default()
    }));
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&program.words);
    for kind in [0, 4] {
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
    assert!(halted, "bounded execution peer timeout: {label}");
    let s = shared.borrow();
    assert!(s.statuses.is_empty(), "unused status script: {label}");
    assert_eq!(
        s.status_reads,
        statuses.len(),
        "unexpected status reads: {label}"
    );
    s.events.clone()
}

#[test]
fn execution_peer_orders_publication_start_and_single_retirement() {
    let events = run(
        "replay",
        r#"
 check(gpu_execute_prepare(0));check(GPU_EXECUTION[1u16]==1);
 check(GPU_EXECUTION[0u16]==GPU_EXEC_PREPARED);check(gpu_execute_poll()==0);
 check(gpu_execute_start(0,false));check(gpu_execute_poll()==0);check(GPU_EXECUTION[1u16]==1);
 check(gpu_execute_poll()==1);check(gpu_execute_poll()==0);check(GPU_EXECUTION[1u16]==0);
 check(gpu_execute_prepare(0));check(gpu_execute_start(0,false));check(gpu_execute_poll()==1);
 check(gpu_execute_prepare(1));check(gpu_execute_start(1,true));check(gpu_execute_poll()==2);
 check(GPU_EXECUTION[5u16]==0);check(gpu_record_data(0)[GPU_RECORD_COMMAND_USED]==8);
 "#,
        &[0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        false,
    );
    assert_eq!(
        events
            .iter()
            .filter(|e| e.0 == 4)
            .copied()
            .collect::<Vec<_>>(),
        vec![
            (4, 0, 3),
            (4, 1, 0x9800),
            (4, 0, 3),
            (4, 1, 0x9800),
            (4, 0, 3),
            (4, 1, 0xfc00)
        ]
    );
    assert_eq!(events.iter().filter(|e| e.0 == 0).count(), 4);
    let order = events
        .iter()
        .map(|e| {
            if e.0 == 0 {
                "publish"
            } else if e.1 == 0 {
                "count"
            } else {
                "start"
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(
        order,
        [
            "publish", "count", "start", "publish", "count", "start", "publish", "publish",
            "count", "start"
        ]
    );
}

#[test]
fn depth_shift_requires_hardware_capability_before_publication_or_start() {
    let events = run(
        "depth-legacy-hardware",
        r#"
 gpu_record_data(0)[GPU_RECORD_VALID]|=GPU_RECORD_NEEDS_DEPTH_SHIFT;
 check(!gpu_execute_prepare(0));check(GPU_EXECUTION[5u16]==9);
 check(GPU_EXECUTION[1u16]==1);check(gpu_execute_poll()==65535);
 "#,
        &[0, 0],
        false,
    );
    assert!(
        events.is_empty(),
        "unsupported depth shift must not publish or SEND"
    );
}

#[test]
fn execution_peer_faults_keep_the_owned_slot_without_extra_sends() {
    let cache = run(
        "cache-error",
        r#"
 check(!gpu_execute_prepare(0));check(GPU_EXECUTION[1u16]==1);check(GPU_EXECUTION[5u16]==6);
 check(gpu_execute_poll()==65535);
 "#,
        &[0, 0],
        true,
    );
    assert!(cache.iter().all(|e| e.0 != 4));
    let busy = run(
        "unexpected-busy",
        r#"
 check(!gpu_execute_prepare(0));check(GPU_EXECUTION[1u16]==1);check(gpu_execute_poll()==65535);
 "#,
        &[0, 1],
        false,
    );
    assert!(busy.is_empty());
    let fault = run(
        "fault-idle",
        r#"
 check(gpu_execute_prepare(0));check(gpu_execute_start(0,false));
 check(gpu_execute_poll()==65535);check(GPU_EXECUTION[1u16]==1);
 check(gpu_execute_poll()==65535);
 "#,
        &[0, 0, 0, 0, 2],
        false,
    );
    assert_eq!(fault.iter().filter(|e| e.0 == 4).count(), 2);
    let duplicate = run(
        "busy-submit",
        r#"
 check(gpu_execute_prepare(0));check(gpu_execute_start(0,false));
 check(!gpu_execute_prepare(1));check(GPU_EXECUTION[1u16]==1);check(gpu_execute_poll()==65535);
 "#,
        &[0, 0, 0, 0],
        false,
    );
    assert_eq!(duplicate.iter().filter(|e| e.0 == 4).count(), 2);
    let changed = run(
        "changed-prepared",
        r#"
 check(gpu_execute_prepare(0));gpu_record_data(0)[GPU_RECORD_COMMAND_BASE]=0x9900;
 check(!gpu_execute_start(0,false));check(GPU_EXECUTION[1u16]==1);check(gpu_execute_poll()==65535);
 "#,
        &[0, 0, 0],
        false,
    );
    assert!(changed.iter().all(|e| e.0 != 4));
}
