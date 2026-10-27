//! Compiled-CPU misuse/fault checks for the diagnostic template experiment.
//! These peers observe control only and do not simulate GPU execution.
use cpu_v3::{rcc_backend, CpuV3Sim, PhysicalWordAddress, RunOutcome};
use std::{any::Any, cell::RefCell, path::Path, rc::Rc};
#[derive(Default)]
struct State {
    starts: usize,
    maintenance: usize,
    status: u16,
    cache_error: bool,
}
struct Peer {
    kind: u8,
    state: Rc<RefCell<State>>,
}
impl cpu_v3::Device for Peer {
    fn read(&mut self, _: &mut [u16], channel: u8) -> u16 {
        let s = self.state.borrow();
        match (self.kind, channel) {
            (3, 2) => 0x20,
            (4, 2) => s.status,
            (0, 5) if s.cache_error && s.maintenance != 0 => 0x8000,
            _ => 0,
        }
    }
    fn write(&mut self, _: &mut [u16], channel: u8, value: u16) {
        let mut s = self.state.borrow_mut();
        match (self.kind, channel) {
            (0, 1) => s.maintenance += 1,
            (4, 1) => s.starts += 1,
            (7, 14) => s.status = value,
            _ => {}
        }
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}
fn rejected(body: &str, cache_error: bool) -> State {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    let source = format!(
        r#"mod template_experiment;
static MATRIX:Buf<u16,45>=Buf::new([0;45]);
static CELLS:Buf<u16,375>=Buf::new([65535;375]);
static BUTTONS:Buf<u16,4>=Buf::new([0;4]);
static STATS:Buf<u16,4>=Buf::new([0;4]);
fn demo_overlay_begin(c:Array<u16>,s:Array<u16>,n:u16,b:bool){{}}
fn demo_buttons_update(s:Array<u16>,r:u16,n:u16)->bool{{true}}
fn demo_report(a:u16,b:u16,c:u16){{}}
fn time_milliseconds()->u16{{0}}
fn main(){{{body} halt(0xbad);}}
"#
    );
    let ir =
        rcc::frontend::compile_program_named("Template controls", &source, &options, &mut |name| {
            if name == "boot_resources" {
                return Ok("pub const DEMO_CAMERA_FIRST:u16=0x9600;".into());
            }
            let local = root
                .join("examples/gpu_demo/rcc")
                .join(format!("{name}.rs"));
            let path = if local.exists() {
                local
            } else {
                root.join("rcc").join(format!("{name}.rs"))
            };
            std::fs::read_to_string(path).map_err(|e| e.to_string())
        })
        .unwrap();
    let program = rcc_backend::try_compile(ir, &options, "main").unwrap();
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&program.words);
    cpu.physical_memory_mut()[0x490000 / 2] = 0x12ab;
    cpu.physical_memory_mut()[0x4a8000 / 2] = 0xab12;
    let state = Rc::new(RefCell::new(State {
        cache_error,
        ..State::default()
    }));
    for kind in [0, 3, 4, 7] {
        cpu.attach_device(
            kind,
            Box::new(Peer {
                kind,
                state: state.clone(),
            }),
        );
    }
    assert!(matches!(
        cpu.run(2_000_000).unwrap(),
        RunOutcome::Halted { signal: 0xe001, .. }
    ));
    assert_eq!(
        cpu.physical_memory(PhysicalWordAddress::new(0x490000 / 2)),
        0x12ab
    );
    assert_eq!(
        cpu.physical_memory(PhysicalWordAddress::new(0x4a8000 / 2)),
        0xab12
    );
    assert_eq!(state.borrow().starts, 0);
    drop(cpu);
    Rc::try_unwrap(state)
        .ok()
        .expect("CPU peers must be dropped")
        .into_inner()
}
#[test]
fn retained_input_slot_cannot_be_modified_until_completion() {
    for state in [2, 3] {
        let s = rejected(&format!("EXP_CB0.state={state};exp_prepare(0,0);"), false);
        assert_eq!(s.maintenance, 0);
    }
    rejected("exp_prepare(2,0);", false);
    rejected(
        "exp_copy(CELLS.as_array(),GPU_COLOR_A_SECTOR,0,1,false);",
        false,
    );
}
#[test]
fn busy_transport_and_cache_faults_prevent_next_submission() {
    let setup="if gpu_init()!=GPU_IDLE{halt(0xbad);}let mut c=exp_buffer(0);c[0u16]=GPU_CB;c[0u16].state=GPU_CB_READY;c[0u16].color=GPU_COLOR_B_SECTOR;c[0u16].command_words=2;";
    for status in [1, 2] {
        rejected(
            &format!("{setup}dev_send(7,14,{status});exp_submit(0);"),
            false,
        );
    }
    let s = rejected(&format!("{setup}exp_submit(0);"), true);
    assert_eq!(s.maintenance, 1);
}

#[test]
#[ignore = "requires the frozen diagnostic artifacts; compilation only, no GPU execution"]
fn current_sources_match_frozen_template_experiments() {
    #[path = "../build_support/rcc_modules.rs"]
    mod resolver;
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let artifacts = root.join("../../target/maintainer/demo-toggle-20261023");
    let constants = std::fs::read_to_string(artifacts.join("boot/boot_resources.rs")).unwrap();
    let mut options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    options.stack_init = cpu_v3_tang_nano_20k::boot::SINGLE_BOOT_STACK;
    for name in ["retained-v3", "pingpong-small-v1", "pingpong-present-v1"] {
        let dir = artifacts.join(format!("cpu-prep-{name}"));
        let source = std::fs::read_to_string(dir.join("program.rs")).unwrap();
        let ir = rcc::frontend::compile_program_named(
            "Classic profile",
            &source,
            &options,
            &mut |name| {
                resolver::load_module(
                    &root.join("examples/gpu_demo/rcc"),
                    &root.join("rcc"),
                    &constants,
                    name,
                    &mut |_| {},
                )
            },
        )
        .unwrap();
        let actual = rcc_backend::try_compile(ir, &options, "demo_one_frame_main").unwrap();
        let frozen: Vec<u16> = std::fs::read_to_string(dir.join("program.hex"))
            .unwrap()
            .lines()
            .map(|line| u16::from_str_radix(line, 16).unwrap())
            .collect();
        assert!(
            actual.words == frozen,
            "{name}: source changed the frozen program; lengths {} / {}",
            actual.words.len(),
            frozen.len()
        );
        println!("SOURCE_IDENTITY_PASS {name} words={}", frozen.len());
    }
}
