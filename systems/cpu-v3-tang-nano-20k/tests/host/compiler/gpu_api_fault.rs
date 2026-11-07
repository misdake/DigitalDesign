//! API fault UART peer: backpressure and final stop-bit drain, entirely offline.
use cpu_v3::{rcc_backend, CpuV3Sim, Device, StepOutcome};
use rcc::frontend::{compile_program_named_checked, rustc_check::RustcCheck};
use std::{any::Any, cell::RefCell, path::Path, rc::Rc};

#[derive(Default)]
pub(super) struct Uart {
    pub(super) bytes: Vec<u8>,
    pub(super) busy: usize,
}
pub(super) struct Peer(pub(super) Rc<RefCell<Uart>>);
impl Device for Peer {
    fn read(&mut self, _: &mut [u16], channel: u8) -> u16 {
        if channel != 3 {
            return 0;
        }
        let mut u = self.0.borrow_mut();
        if u.busy == 0 {
            0
        } else {
            u.busy -= 1;
            1
        }
    }
    fn write(&mut self, _: &mut [u16], channel: u8, value: u16) {
        assert_eq!(channel, 3);
        let mut u = self.0.borrow_mut();
        assert_eq!(u.busy, 0, "API fault ignored UART backpressure");
        assert!(value < 256);
        u.bytes.push(value as u8);
        u.busy = 2;
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[test]
fn extent_validation_and_first_fault_execute_in_both_profiles() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("rcc");
    let out = std::env::var_os("RCC_RUSTC_TEST_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("../../../target/graphics-api-g0"));
    for enabled in [false, true] {
        for (label, body, expected) in [
            (
                "valid",
                r#"let memory:GpuMemory=GpuMemory{first_sector:65535,sector_count:1};
if !gpu_memory_extent_valid(65535,1)||gpu_memory_extent_valid(65535,2)||gpu_memory_extent_valid(0,0){halt(0xbad);}
if memory.first_sector()!=65535||memory.sector_count()!=1{halt(0xbad);}halt(0x600d);"#,
                Vec::new(),
            ),
            (
                "overflow",
                r#"if !gpu_memory_extent_valid(65535,2){gpu_api_fail(1,1,65535,2);}halt(0xbad);"#,
                vec![0xfa17, 1, 1, 65535, 2, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            ),
            (
                "zero",
                r#"if !gpu_memory_extent_valid(0,0){gpu_api_fail(1,1,0,0);}halt(0xbad);"#,
                vec![0xfa17, 1, 1, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            ),
            (
                "sticky",
                r#"GPU_API_FAULT[0u16]=0xfa17;GPU_API_FAULT[1u16]=7;GPU_API_FAULT[2u16]=8;
gpu_api_fail(10,11,12,13);halt(0xbad);"#,
                vec![0xfa17, 7, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            ),
        ] {
            let mut options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
            options.gpu_debug = enabled;
            let source = format!("mod gpu_schema;mod gpu_api_fault;fn main(){{{body}}}");
            let parsed = compile_program_named_checked(
                "api-fault-peer",
                &source,
                &options,
                &RustcCheck::new(out.join(format!("fault-{label}-{enabled}"))),
                &mut |name| {
                    std::fs::read_to_string(root.join(format!("{name}.rs")))
                        .map_err(|e| e.to_string())
                },
            )
            .unwrap();
            let program = rcc_backend::try_compile(parsed, &options, "main").unwrap();
            let shared = Rc::new(RefCell::new(Uart::default()));
            let mut cpu = CpuV3Sim::default();
            cpu.set_boot_window(&program.words);
            cpu.attach_device(0, Box::new(Peer(shared.clone())));
            let mut halted = false;
            for _ in 0..500_000 {
                if let StepOutcome::Halted { signal } = cpu.step().unwrap() {
                    assert_eq!(
                        signal,
                        if expected.is_empty() { 0x600d } else { 0xe001 },
                        "{label}/{enabled}"
                    );
                    halted = true;
                    break;
                }
            }
            assert!(halted, "bounded API fault timeout: {label}/{enabled}");
            let uart = shared.borrow();
            assert_eq!(uart.busy, 0, "halt before last stop bit: {label}/{enabled}");
            let bytes = expected
                .iter()
                .flat_map(|w: &u16| w.to_le_bytes())
                .collect::<Vec<_>>();
            assert_eq!(uart.bytes, bytes, "{label}/{enabled}");
        }
    }
}
