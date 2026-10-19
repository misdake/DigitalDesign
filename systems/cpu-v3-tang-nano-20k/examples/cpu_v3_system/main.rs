use cpu_v3::{CpuV3Core, CpuV3DataCache, CpuV3InstructionCache, CpuV3InstructionFetchQueue};
use cpu_v3_tang_nano_20k::display::ACTIVE_DISPLAY_CONFIG;
use cpu_v3_tang_nano_20k::{
    CpuV3Gpu, CpuV3MemoryArbiter, FramebufferHdmi, SharedSdramPort, SingleBootLoader,
    SystemControlDevice,
};
use digital_design_circuit::CircuitWires;
use digital_design_hardware::{Hardware, HardwareIdentity, Module, VerilogDependency};
use digital_design_hardware_gowin::{
    run_gowin_project_cli, ErasedSpiFlashImage, GowinCliError, GowinModuleProject,
    ResourceCountExpectation, SpiFlashReader, TangNano20K, TangNano20KBootHdmiWideInputs,
    TangNano20KBootHdmiWideOutputs, TangNano20KVideoMode,
};
use std::cell::{Cell, RefCell};
const GENERATED_APPLICATION_PACKAGE: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/cpu-v3-boot.bin"));
type FittedFlashReader = SpiFlashReader<ErasedSpiFlashImage, 8_388_608, 2>;
type SystemControl = SystemControlDevice<469>;
thread_local! {static TEST_IMAGE:RefCell<Option<(Vec<u8>,bool,u16)>>=const{RefCell::new(None)};}
thread_local! {static BOARD_APPLICATION_TEST:Cell<bool>=const{Cell::new(false)};}
fn main() -> Result<(), GowinCliError> {
    run_gowin_project_cli(gowin_project(), "target/cpu_v3_system_gowin")
}
#[derive(Hardware)]
#[hardware(namespace = "examples/cpu_v3_system")]
struct CpuV3System;
impl Module for CpuV3System {
    type Input = TangNano20KBootHdmiWideInputs;
    type Output = TangNano20KBootHdmiWideOutputs;
    type EmuState = ();
    const USES_MAIN_CLOCK: bool = true;
    const EMU_AVAILABLE: bool = false;
    fn execute_emu(_: &mut (), _: &mut CircuitWires, _: &Self::Input, _: &Self::Output) {
        panic!("Verilog-only system integration")
    }
    fn verilog_source() -> Option<String> {
        let mut text = include_str!("self_test.v").to_string();
        for (key, name) in [
            (
                "__CPU_V3_CORE__",
                CpuV3Core::verilog_identity().module_name(),
            ),
            (
                "__FETCH_QUEUE__",
                CpuV3InstructionFetchQueue::verilog_identity().module_name(),
            ),
            (
                "__CACHE__",
                CpuV3InstructionCache::verilog_identity().module_name(),
            ),
            (
                "__DATA_CACHE__",
                CpuV3DataCache::verilog_identity().module_name(),
            ),
            (
                "__ARBITER__",
                CpuV3MemoryArbiter::verilog_identity().module_name(),
            ),
            ("__GPU__", CpuV3Gpu::verilog_identity().module_name()),
            (
                "__SYSTEM_CONTROL__",
                SystemControl::verilog_identity().module_name(),
            ),
            (
                "__SINGLE_BOOT_LOADER__",
                SingleBootLoader::verilog_identity().module_name(),
            ),
            (
                "__FLASH_READER__",
                FittedFlashReader::verilog_identity().module_name(),
            ),
            (
                "__SHARED_SDRAM_PORT__",
                SharedSdramPort::verilog_identity().module_name(),
            ),
            (
                "__FRAMEBUFFER_HDMI__",
                FramebufferHdmi::verilog_identity().module_name(),
            ),
        ] {
            text = text.replace(key, &name);
        }
        Some(text)
    }
    fn verilog_dependencies() -> Vec<VerilogDependency> {
        vec![
            VerilogDependency::new::<CpuV3Core>("u_core"),
            VerilogDependency::new::<CpuV3InstructionFetchQueue>("u_instruction_fetch_queue"),
            VerilogDependency::new::<CpuV3InstructionCache>("u_instruction_cache"),
            VerilogDependency::new::<CpuV3DataCache>("u_data_cache"),
            VerilogDependency::new::<CpuV3MemoryArbiter>("u_memory_arbiter"),
            VerilogDependency::new::<CpuV3Gpu>("u_gpu"),
            VerilogDependency::new::<SystemControl>("u_sysctl"),
            VerilogDependency::new::<SingleBootLoader>("u_boot_loader"),
            VerilogDependency::new::<FittedFlashReader>("u_flash"),
            VerilogDependency::new::<SharedSdramPort>("u_shared_sdram_port"),
            VerilogDependency::new::<FramebufferHdmi>("u_display"),
        ]
    }
    fn verilog_testbench() -> Option<String> {
        if BOARD_APPLICATION_TEST.with(Cell::get) {
            let fixture = TEST_IMAGE.with(|x| x.borrow().clone());
            let (package, _, halt) =
                fixture.unwrap_or_else(|| (GENERATED_APPLICATION_PACKAGE.to_vec(), false, 0));
            let mut init = String::new();
            for (index, byte) in package.iter().enumerate() {
                init.push_str(&format!(" flash_image[{index}]=8'h{byte:02x};\n"));
            }
            let text = include_str!("board_application_tb.v")
                .replace("__PACKAGE_BYTES__", &package.len().to_string())
                .replace("__PACKAGE_INIT__", &init)
                .replace("__PROBE_HALT__", &halt.to_string());
            let evidence = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/system-board-ready/audit/native-progress.log");
            let evidence = if halt != 0 {
                evidence.with_file_name("native-maintenance-progress.log")
            } else {
                evidence
            };
            std::fs::create_dir_all(evidence.parent().unwrap()).unwrap();
            let text = text.replace(
                "__PROGRESS_PATH__",
                &evidence.to_string_lossy().replace('\\', "/"),
            );
            use digital_design_hardware_gowin::sdram_memory_controller::RtlSources;
            // The shared test runner selects Verilog-2005. Preserve the pin
            // model's unknown-data assertion without the SystemVerilog task.
            let pins = RtlSources::PIN_MODEL.replace("$isunknown(dq)", "((^dq) === 1'bx)");
            return Some(format!(
                "{text}\n{}\n{}\n{}",
                RtlSources::GEARBOX,
                RtlSources::CONTROLLER,
                pins
            ));
        }
        let fixture = TEST_IMAGE.with(|x| x.borrow().clone());
        let (package, error, halt) =
            fixture.unwrap_or_else(|| (GENERATED_APPLICATION_PACKAGE.to_vec(), false, 0));
        let mut flash_init = String::new();
        let mut code_init = String::new();
        for (index, byte) in package.iter().enumerate() {
            flash_init.push_str(&format!(" flash_image[{index}]=8'h{byte:02x};\n"));
        }
        let code = &package[128..];
        for (index, word) in code.as_chunks::<2>().0.iter().enumerate() {
            let value = u16::from_le_bytes([word[0], word[1]]);
            code_init.push_str(&format!(" expected_code[{index}]=16'h{value:04x};\n"));
        }
        let receipt = std::env::var_os("SINGLE_BOOT_EVIDENCE")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join(format!("startup-{halt:04x}-{}.json", u8::from(error)));
        Some(
            include_str!("signature_testbench.v")
                .replace(
                    "__STARTUP_RESULT_PATH__",
                    &receipt.to_string_lossy().replace('\\', "/"),
                )
                .replace("__FLASH_PACKAGE_SIZE__", &package.len().to_string())
                .replace("__FLASH_PACKAGE_INIT__", &flash_init)
                .replace("__CODE_WORDS__", &(code.len() / 2).to_string())
                .replace("__CODE_INIT__", &code_init)
                .replace("__EXPECT_ERROR__", &u8::from(error).to_string())
                .replace("__EXPECT_HALT__", &halt.to_string()),
        )
    }
}
fn gowin_project() -> GowinModuleProject<TangNano20K, CpuV3System> {
    let mode = TangNano20KVideoMode::from_pixel_clock(ACTIVE_DISPLAY_CONFIG.pixel_clock_hz);
    TangNano20K::cold_boot_hdmi_memory_project::<CpuV3System>("cpu_v3_system", mode)
        .with_timing_driven_implementation()
        .expect_bsram_blocks(ResourceCountExpectation::Claimed)
}
#[cfg(test)]
mod tests {
    use super::*;
    use cpu_v3_tang_nano_20k::boot::pack_single_application;
    use digital_design_hardware::{ResourceKind, VerilogProject};
    #[test]
    #[ignore = "bounded CPU-only application, real SPI, caches, arbiter, gear, native controller, SDRAM pins and Display"]
    fn board_application_with_native_sdram_in_verilog() {
        BOARD_APPLICATION_TEST.with(|x| x.set(true));
        let result = digital_design_hardware::verify_verilog_with_iverilog::<CpuV3System>();
        BOARD_APPLICATION_TEST.with(|x| x.set(false));
        result.unwrap();
    }
    #[test]
    #[ignore = "bounded native SDRAM source-dirty LCOPY followed by full clean/invalidate"]
    fn native_sdram_maintenance_probe_in_verilog() {
        let source = r#"
            fn fail(status: u16) { dev_send(0,2,status); halt(0xff01); }
            fn invalidate() {
                unsafe { dcache_clean_line(Ptr::from_addr(0x4000)); } dcache_wait();
                if dcache_invalidate_all()!=0 { fail(1); }
            }
            fn main() {
                let mut line: u16=0;
                while line<256 {
                    let p=Ptr::from_addr(0x4000+(line<<4));
                    let mut i: u16=0;
                    while i<16 { unsafe { p.write(i as i16,0x1234); } i+=1; }
                    unsafe { dcache_line_copy(p,128); } dcache_wait(); line+=1;
                }
                invalidate();
                halt(0x5a35);
            }
        "#;
        let options = cpu_v3::rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
        let parsed = rcc::frontend::compile_program_named(
            "native-maintenance-probe.rs",
            source,
            &options,
            &mut |_| Err("unexpected module".into()),
        )
        .unwrap();
        let program = cpu_v3::rcc_backend::try_compile(parsed, &options, "main").unwrap();
        let evidence = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/system-board-ready/audit");
        std::fs::create_dir_all(&evidence).unwrap();
        std::fs::write(
            evidence.join("native-maintenance-probe.lst"),
            &program.listing,
        )
        .unwrap();
        std::fs::write(evidence.join("native-maintenance-probe.rcc"), source).unwrap();
        let bytes: Vec<_> = program.words.iter().flat_map(|x| x.to_le_bytes()).collect();
        let package = pack_single_application(&bytes).unwrap();
        TEST_IMAGE.with(|x| *x.borrow_mut() = Some((package, false, 0x5a35)));
        BOARD_APPLICATION_TEST.with(|x| x.set(true));
        let result = digital_design_hardware::verify_verilog_with_iverilog::<CpuV3System>();
        BOARD_APPLICATION_TEST.with(|x| x.set(false));
        TEST_IMAGE.with(|x| *x.borrow_mut() = None);
        result.unwrap();
    }
    fn fixture() -> Vec<u8> {
        let source = include_str!("../../rcc/boot-contract.rs");
        let mut options =
            cpu_v3::rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
        options.stack_init = 0xc000;
        let program = rcc::frontend::compile_program_named(
            "boot-contract.rs",
            source,
            &options,
            &mut |name| Err(format!("unexpected module {name}")),
        )
        .unwrap();
        let program = cpu_v3::rcc_backend::try_compile(program, &options, "main").unwrap();
        let bytes: Vec<u8> = program.words.iter().flat_map(|x| x.to_le_bytes()).collect();
        pack_single_application(&bytes).unwrap()
    }
    #[test]
    fn single_application_assets_repack_exactly() {
        assert_eq!(
            pack_single_application(include_bytes!(concat!(
                env!("OUT_DIR"),
                "/application.v3bin"
            )))
            .unwrap(),
            GENERATED_APPLICATION_PACKAGE
        );
    }
    #[test]
    fn application_updates_do_not_change_gateware() {
        let before = VerilogProject::generate::<CpuV3System>().unwrap().files;
        TEST_IMAGE.with(|x| *x.borrow_mut() = Some((fixture(), false, 0x5a35)));
        let after = VerilogProject::generate::<CpuV3System>().unwrap().files;
        TEST_IMAGE.with(|x| *x.borrow_mut() = None);
        assert_eq!(before, after);
    }
    #[test]
    fn full_system_omits_rom_overlay_dma_and_runtime_reset() {
        let project = VerilogProject::generate::<CpuV3System>().unwrap();
        let text = project
            .files
            .values()
            .cloned()
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            !text.contains("BootDmaEngine")
                && !text.contains("ResetController")
                && !text.contains("instruction_source_count")
                && !text.contains("u_boot (")
        );
        let board = gowin_project().generate().unwrap();
        assert_eq!(board.resources.claimed[&ResourceKind::Bsram18K], 10);
    }
    #[test]
    #[ignore = "complete real-SPI production application cold boot"]
    fn flash_boot_executes_in_verilog() {
        digital_design_hardware::verify_verilog_with_iverilog::<CpuV3System>().unwrap();
    }
    #[test]
    #[ignore = "complete real-SPI nonzero memory / zero-and-nonzero-static fixture"]
    fn flash_boot_statics_in_verilog() {
        TEST_IMAGE.with(|x| *x.borrow_mut() = Some((fixture(), false, 0x5a35)));
        let result = digital_design_hardware::verify_verilog_with_iverilog::<CpuV3System>();
        TEST_IMAGE.with(|x| *x.borrow_mut() = None);
        result.unwrap();
    }
    #[test]
    #[ignore = "malformed image must never release CPU or write a device"]
    fn flash_boot_rejects_header_in_verilog() {
        let mut bad = fixture();
        bad[0] ^= 1;
        TEST_IMAGE.with(|x| *x.borrow_mut() = Some((bad, true, 0)));
        let result = digital_design_hardware::verify_verilog_with_iverilog::<CpuV3System>();
        TEST_IMAGE.with(|x| *x.borrow_mut() = None);
        result.unwrap();
    }
}
