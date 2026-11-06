//! Production board composition; the Cargo example is only its stable CLI.
use cpu_v3::{CpuV3Core, CpuV3DataCache, CpuV3InstructionCache, CpuV3InstructionFetchQueue};
use cpu_v3_tang_nano_20k::display::ACTIVE_DISPLAY_CONFIG;
use cpu_v3_tang_nano_20k::{
    CpuV3Gpu, CpuV3MemoryArbiter, DualShock2, FramebufferHdmi, SingleBootLoader,
    SystemControlDevice,
};
use digital_design_circuit::CircuitWires;
use digital_design_hardware::{Hardware, HardwareIdentity, Module, VerilogDependency};
use digital_design_hardware_gowin::sdram_memory_controller::StreamingSdramPort;
use digital_design_hardware_gowin::{
    ErasedSpiFlashImage, GowinModuleProject, ResourceCountExpectation, SpiFlashReader, TangNano20K,
    TangNano20KBootHdmiStreamDs2Inputs, TangNano20KBootHdmiStreamDs2Outputs, TangNano20KVideoMode,
};
use std::cell::{Cell, RefCell};
const GENERATED_APPLICATION_PACKAGE: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/cpu-v3-boot.bin"));
// The sealed board baseline stays at 2. Set 1 only for the 27 MHz candidate.
const SPI_HALF_PERIOD: u64 = match option_env!("CPU_V3_SPI_HALF_PERIOD") {
    None => 2,
    Some(value) => {
        let bytes = value.as_bytes();
        assert!(bytes.len() == 1 && (bytes[0] == b'1' || bytes[0] == b'2'));
        (bytes[0] - b'0') as u64
    }
};
type FittedFlashReader = SpiFlashReader<ErasedSpiFlashImage, 8_388_608, SPI_HALF_PERIOD>;
type SystemControl = SystemControlDevice<469, 54000>;
thread_local! {static TEST_IMAGE:RefCell<Option<(Vec<u8>,bool,u16)>>=const{RefCell::new(None)};}
thread_local! {static TEST_ERROR_CODE:Cell<u8>=const{Cell::new(1)};}
thread_local! {static BOARD_APPLICATION_TEST:Cell<bool>=const{Cell::new(false)};}
thread_local! {static GPU_COLD_TEST:Cell<bool>=const{Cell::new(false)};}
#[derive(Hardware)]
#[hardware(namespace = "examples/cpu_v3_system")]
pub(crate) struct CpuV3System;
impl Module for CpuV3System {
    type Input = TangNano20KBootHdmiStreamDs2Inputs;
    type Output = TangNano20KBootHdmiStreamDs2Outputs;
    type EmuState = ();
    const USES_MAIN_CLOCK: bool = true;
    const EMU_AVAILABLE: bool = false;
    fn execute_emu(_: &mut (), _: &mut CircuitWires, _: &Self::Input, _: &Self::Output) {
        panic!("Verilog-only system integration")
    }
    fn verilog_source() -> Option<String> {
        let mut text = include_str!("top.v").to_string();
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
            ("__DS2__", DualShock2::verilog_identity().module_name()),
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
                StreamingSdramPort::verilog_identity().module_name(),
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
            VerilogDependency::new::<DualShock2>("u_ds2"),
            VerilogDependency::new::<SystemControl>("u_sysctl"),
            VerilogDependency::new::<SingleBootLoader>("u_boot_loader"),
            VerilogDependency::new::<FittedFlashReader>("u_flash"),
            VerilogDependency::new::<StreamingSdramPort>("u_shared_sdram_port"),
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
                .join("../../target/cb06-validation/native-progress.log");
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
            let models =
                std::path::PathBuf::from(std::env::var_os("GOWIN_HOME").expect("GOWIN_HOME"))
                    .join("IDE/simlib/gw2a/prim_sim.v");
            return Some(format!(
                "{text}\n{}\n{}\n{}\n{}",
                RtlSources::STREAM_GEARBOX,
                RtlSources::CONTROLLER,
                pins,
                std::fs::read_to_string(models).unwrap()
            ));
        }
        let fixture = TEST_IMAGE.with(|x| x.borrow().clone());
        let (package, error, halt) =
            fixture.unwrap_or_else(|| (GENERATED_APPLICATION_PACKAGE.to_vec(), false, 0));
        let mut flash_init = String::new();
        let mut code_init = String::new();
        if package.len() <= 65536 {
            for (index, byte) in package.iter().enumerate() {
                flash_init.push_str(&format!(" flash_image[{index}]=8'h{byte:02x};\n"));
            }
        }
        if package.len() > 65536 {
            let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/cb06-validation/flash");
            std::fs::create_dir_all(&dir).unwrap();
            let hash = package.iter().fold(0xcbf29ce484222325u64, |h, b| {
                (h ^ u64::from(*b)).wrapping_mul(0x100000001b3)
            });
            let file = dir.join(format!("{hash:016x}-spi{SPI_HALF_PERIOD}.hex"));
            let mut hex = String::with_capacity(package.len() * 3);
            for byte in &package {
                use std::fmt::Write;
                writeln!(hex, "{byte:02x}").unwrap();
            }
            std::fs::write(&file, hex).unwrap();
            flash_init = format!(
                "$readmemh(\"{}\",flash_image);\n",
                file.to_string_lossy().replace('\\', "/")
            );
        }
        let mut resource_check = String::new();
        if let Ok(commands) = cpu_v3_tang_nano_20k::boot::single_boot_commands(&package) {
            for command in commands {
                if let Some(data) = command.payload {
                    if command.address == 0xe0000 {
                        continue;
                    }
                    let offset = data.as_ptr() as usize - package.as_ptr() as usize;
                    resource_check.push_str(&format!("for(n=0;n<{};n=n+1)if(memory[{}+n]!=={{flash_image[{}+2*n+1],flash_image[{}+2*n]}})$fatal(1,\"resource COPY mismatch %0d\",n);\n",command.length/2,command.address/2,offset,offset));
                }
            }
        }
        let static_words = cpu_v3_tang_nano_20k::boot::single_boot_commands(&package)
            .unwrap_or_default()
            .iter()
            .find(|c| c.address == 0 && c.payload.is_none())
            .map_or(0, |c| c.length / 2);
        let code = cpu_v3_tang_nano_20k::boot::single_boot_code(&package).unwrap_or(&[]);
        for (index, word) in code.as_chunks::<2>().0.iter().enumerate() {
            let value = u16::from_le_bytes([word[0], word[1]]);
            code_init.push_str(&format!(" expected_code[{index}]=16'h{value:04x};\n"));
        }
        let receipt = std::env::var_os("SINGLE_BOOT_EVIDENCE")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join(format!(
                "startup-{halt:04x}-{}-spi{SPI_HALF_PERIOD}-{}.json",
                u8::from(error),
                package.len()
            ));
        std::fs::create_dir_all(receipt.parent().unwrap()).unwrap();
        let mut testbench = include_str!("signature_testbench.v")
            .replace(
                "__STARTUP_RESULT_PATH__",
                &receipt.to_string_lossy().replace('\\', "/"),
            )
            .replace("__FLASH_PACKAGE_SIZE__", &package.len().to_string())
            .replace("__FLASH_PACKAGE_INIT__", &flash_init)
            .replace("__STATIC_WORDS__", &static_words.to_string())
            .replace("__CODE_WORDS__", &(code.len() / 2).to_string())
            .replace("__CODE_INIT__", &code_init)
            .replace("__RESOURCE_CHECK__", &resource_check)
            .replace("__SPI_HALF_PERIOD__", &SPI_HALF_PERIOD.to_string())
            .replace(
                "__GPU_COLD_FIXTURE__",
                &u8::from(GPU_COLD_TEST.with(Cell::get)).to_string(),
            )
            .replace(
                "__FRAMEBUFFER_START_WORD__",
                &(cpu_v3_tang_nano_20k::boot::BOOT_FRAMEBUFFER_BYTES.start / 2).to_string(),
            )
            .replace(
                "__FRAMEBUFFER_END_WORD__",
                &(cpu_v3_tang_nano_20k::boot::BOOT_FRAMEBUFFER_BYTES.end / 2).to_string(),
            )
            .replace(
                "__BOOT_MAX_CYCLES__",
                &if GPU_COLD_TEST.with(Cell::get) {
                    // Focused Device4 fixture: real SPI/code/static loading,
                    // virtual-black startup and an independent video pre-roll.
                    let words: usize = cpu_v3_tang_nano_20k::boot::single_boot_commands(&package)
                        .unwrap()
                        .iter()
                        .map(|command| command.length / 2)
                        .sum();
                    (words * 7 + package.len() * 40 + 20_000).max(150_000)
                } else {
                    6_000_000 + package.len() * 40
                }
                .to_string(),
            )
            .replace("__EXPECT_ERROR__", &u8::from(error).to_string())
            .replace(
                "__EXPECT_ERROR_CODE__",
                &TEST_ERROR_CODE.with(Cell::get).to_string(),
            )
            .replace("__EXPECT_HALT__", &halt.to_string());
        testbench = testbench.replacen("module tb;", "module tb;\nGSR GSR(.GSRI(1'b1));", 1);
        if GPU_COLD_TEST.with(Cell::get) {
            testbench = testbench.replacen(
                "endmodule",
                &format!("{}\nendmodule", include_str!("gpu_cold_monitor.vh")),
                1,
            );
            let result_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/gpu-device-cold/production-cpu.json");
            std::fs::create_dir_all(result_path.parent().unwrap()).unwrap();
            testbench = testbench.replace(
                "__GPU_COLD_RESULT_PATH__",
                &result_path.to_string_lossy().replace('\\', "/"),
            );
            testbench = testbench.replace(
                "if(EXPECT_HALT!=0 && dut.halted)begin",
                "if(EXPECT_HALT!=0 && dut.halted)begin\n gpu_cold_complete();",
            );
        }
        // The production GPU exists in every composed system fixture, including
        // COPY/ZERO-only boot tests. All of them require its vendor primitives.
        let models = std::path::PathBuf::from(std::env::var_os("GOWIN_HOME").expect("GOWIN_HOME"))
            .join("IDE/simlib/gw2a/prim_sim.v");
        testbench.push_str(&std::fs::read_to_string(models).unwrap());
        Some(testbench)
    }
}
pub(crate) fn gowin_project() -> GowinModuleProject<TangNano20K, CpuV3System> {
    let mode = TangNano20KVideoMode::from_pixel_clock(ACTIVE_DISPLAY_CONFIG.pixel_clock_hz);
    TangNano20K::cold_boot_hdmi_stream_ds2_memory_project::<CpuV3System>("cpu_v3_system", mode)
        .expect_bsram_blocks(ResourceCountExpectation::Claimed)
}

#[cfg(test)]
mod tests;
