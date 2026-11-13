//! Actual configuration helpers; this is not the public resource Gpu::init gate.
use cpu_v3::{rcc_backend, CpuV3Sim, Device, StepOutcome};
use rcc::frontend::rustc_check::RustcCheck;
use std::{any::Any, cell::RefCell, path::Path, rc::Rc};

#[derive(Default)]
struct State {
    reads: usize,
}
struct Peer {
    kind: u8,
    state: Rc<RefCell<State>>,
}
impl Device for Peer {
    fn read(&mut self, _: &mut [u16], channel: u8) -> u16 {
        assert_eq!(
            (self.kind, channel),
            (4, 2),
            "Gpu init must not query Display"
        );
        self.state.borrow_mut().reads += 1;
        0
    }
    fn write(&mut self, _: &mut [u16], _: u8, _: u16) {
        panic!("Config init must not SEND");
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[test]
fn configuration_checks_all_backing_before_mutation_and_issues_each_buffer_once() {
    let source = r#"mod gpu_config;
static STEP:u16=0;static SURFACE:Buf<u16,16>=Buf::new([0;16]);
fn check(ok:bool){if !ok{halt(0xd000+STEP);}unsafe{addr_of(&STEP).write(0,STEP+1);}}
fn main(){
 let absent:Option<CommandMemory>=None;
 let a:CommandMemory=CommandMemory{commands:GpuMemory{first_sector:0x9800,sector_count:512},push:GpuMemory{first_sector:0x9a00,sector_count:512}};
 let b:CommandMemory=CommandMemory{commands:GpuMemory{first_sector:0xfc00,sector_count:512},push:GpuMemory{first_sector:0xfe00,sector_count:512}};
 let overlap:CommandMemory=CommandMemory{commands:GpuMemory{first_sector:0x99ff,sector_count:1},push:GpuMemory{first_sector:0xfe00,sector_count:512}};
 let invalid:GpuConfig=GpuConfig{object_arena_bytes:8192,command_a:a,command_b:Some(overlap)};
 check(!gpu_config_init(invalid));check(GPU_EXECUTION[0u16]==0&&gpu_record_data(0)[GPU_RECORD_PHASE]==0);
 let too_large:CommandMemory=CommandMemory{commands:GpuMemory{first_sector:0x9800,sector_count:513},push:GpuMemory{first_sector:0x9b00,sector_count:512}};
 let invalid:GpuConfig=GpuConfig{object_arena_bytes:8192,command_a:too_large,command_b:absent};
 check(!gpu_config_init(invalid));check(GPU_EXECUTION[0u16]==0);
 let config:GpuConfig=GpuConfig{object_arena_bytes:8192,command_a:a,command_b:Some(b)};
 check(gpu_config_init(config));check(GPU_EXECUTION[GPU_EXEC_CONFIGURED]==2);
 check(gpu_config_take_buffer()==0);GPU_EXECUTION[0u16]=GPU_EXEC_RUNNING;GPU_EXECUTION[1u16]=1;
 check(gpu_config_take_buffer()==1);check(gpu_config_take_buffer()==65535);check(!gpu_config_drop_buffer(0));
 check(gpu_config_drop_buffer(1));check(gpu_config_take_buffer()==65535);
 GPU_EXECUTION[0u16]=GPU_EXEC_IDLE;GPU_EXECUTION[1u16]=0;
 let config:GpuConfig=GpuConfig{object_arena_bytes:32,command_a:a,command_b:absent};
 check(gpu_config_init(config));check(GPU_EXECUTION[GPU_EXEC_CONFIGURED]==1&&GPU_EXECUTION[GPU_EXEC_TAKEN]==0);
 check(gpu_config_take_buffer()==0&&gpu_config_take_buffer()==65535);check(gpu_record_data(1)[GPU_RECORD_PHASE]==0);
 let memory:GpuMemory=GpuMemory{first_sector:0x8000,sector_count:2048};check(gpu_surface_init(SURFACE.as_array(),memory));
 check(SURFACE[0u16]==0x8000&&SURFACE[1u16]==2048&&SURFACE[15u16]==0);
 let bad:GpuMemory=GpuMemory{first_sector:0x8801,sector_count:2048};check(!gpu_surface_init(SURFACE.as_array(),bad));
 let short:GpuMemory=GpuMemory{first_sector:0x8800,sector_count:1599};check(!gpu_surface_init(SURFACE.as_array(),short));
 check(SURFACE[0u16]==0x8000&&SURFACE[1u16]==2048);halt(0x600d);}"#;
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("rcc");
    let output = std::env::var_os("RCC_RUSTC_TEST_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("../../../target/graphics-api-g0"));
    for enabled in [false, true] {
        let mut options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
        options.gpu_debug = enabled;
        let parsed = crate::checked_flat_fixture::compile(
            "api-config",
            source,
            &options,
            &RustcCheck::new(output.join(format!("config-{enabled}"))),
            &mut |name| {
                std::fs::read_to_string(root.join(format!("{name}.rs"))).map_err(|e| e.to_string())
            },
        )
        .unwrap();
        let program = rcc_backend::try_compile(parsed, &options, "main").unwrap();
        let shared = Rc::new(RefCell::new(State::default()));
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
                    "profile{enabled}, check {}",
                    signal.wrapping_sub(0xd000)
                );
                halted = true;
                break;
            }
        }
        assert!(halted, "bounded config timeout");
        assert_eq!(shared.borrow().reads, 2);
    }
}
