use cpu_v3::{CpuV3Core, CpuV3DataCache, CpuV3InstructionCache, CpuV3InstructionFetchQueue};
use cpu_v3_tang_nano_20k::display::ACTIVE_DISPLAY_CONFIG;
use cpu_v3_tang_nano_20k::{
    BootDmaDevice, BootDmaEngine, BootProgressMonitor, CpuV3MemoryArbiter, FramebufferHdmi,
    SharedSdramPort, SystemControlDevice,
};
use digital_design_circuit::CircuitWires;
use digital_design_hardware::{Hardware, HardwareIdentity, Module, VerilogDependency};
use digital_design_hardware_common::ResetController;
use digital_design_hardware_gowin::{
    run_gowin_project_cli, Bsram1R1Rw1024, BsramImage, ErasedSpiFlashImage, GowinCliError,
    GowinDspMode, GowinModuleProject, ResourceCountExpectation, SpiFlashReader, TangNano20K,
    TangNano20KBootHdmiWideInputs, TangNano20KBootHdmiWideOutputs, TangNano20KVideoMode,
    BSRAM_1024_DEPTH,
};

fn main() -> Result<(), GowinCliError> {
    run_gowin_project_cli(gowin_project(), "target/cpu_v3_system_gowin")
}

include!(concat!(env!("OUT_DIR"), "/boot_images.rs"));

struct BootImage;

const fn boot_image() -> [u64; BSRAM_1024_DEPTH] {
    let mut words = [0; BSRAM_1024_DEPTH];
    let mut index = 0;
    while index < words.len() {
        words[index] = (((index as u64) * 0x9e37) ^ 0x5aa5) & 0xffff;
        index += 1;
    }
    index = 0;
    while index < STAGE0_PROGRAM.len() {
        words[index] = STAGE0_PROGRAM[index] as u64;
        index += 1;
    }
    words
}

impl BsramImage<16> for BootImage {
    const WORDS: [u64; BSRAM_1024_DEPTH] = boot_image();
}

type BootMemory = Bsram1R1Rw1024<16, BootImage>;
type FittedFlashReader = SpiFlashReader<ErasedSpiFlashImage, 8_388_608, 2>;
type SystemControl = SystemControlDevice<469>;
type BoardReset = ResetController<8>;

#[derive(Hardware)]
#[hardware(namespace = "examples/cpu_v3_system")]
struct CpuV3System;

impl Module for CpuV3System {
    type Input = TangNano20KBootHdmiWideInputs;
    type Output = TangNano20KBootHdmiWideOutputs;
    type EmuState = ();

    const USES_MAIN_CLOCK: bool = true;
    const EMU_AVAILABLE: bool = false;

    fn execute_emu(
        _state: &mut Self::EmuState,
        _circuit: &mut CircuitWires,
        _input: &Self::Input,
        _output: &Self::Output,
    ) {
        panic!("CpuV3System is a Verilog-only hardware integration test")
    }

    fn verilog_source() -> Option<String> {
        Some(
            include_str!("self_test.v")
                .replace(
                    "__BOOT_MEMORY__",
                    &BootMemory::verilog_identity().module_name(),
                )
                .replace(
                    "__CPU_V3_CORE__",
                    &CpuV3Core::verilog_identity().module_name(),
                )
                .replace(
                    "__FETCH_QUEUE__",
                    &CpuV3InstructionFetchQueue::verilog_identity().module_name(),
                )
                .replace(
                    "__CACHE__",
                    &CpuV3InstructionCache::verilog_identity().module_name(),
                )
                .replace(
                    "__DATA_CACHE__",
                    &CpuV3DataCache::verilog_identity().module_name(),
                )
                .replace(
                    "__ARBITER__",
                    &CpuV3MemoryArbiter::verilog_identity().module_name(),
                )
                .replace(
                    "__SYSTEM_CONTROL__",
                    &SystemControl::verilog_identity().module_name(),
                )
                .replace(
                    "__BOOT_DMA_DEVICE__",
                    &BootDmaDevice::verilog_identity().module_name(),
                )
                .replace(
                    "__BOOT_DMA_ENGINE__",
                    &BootDmaEngine::verilog_identity().module_name(),
                )
                .replace(
                    "__FLASH_READER__",
                    &FittedFlashReader::verilog_identity().module_name(),
                )
                .replace(
                    "__SHARED_SDRAM_PORT__",
                    &SharedSdramPort::verilog_identity().module_name(),
                )
                .replace(
                    "__FRAMEBUFFER_HDMI__",
                    &FramebufferHdmi::verilog_identity().module_name(),
                )
                .replace(
                    "__RESET_CONTROLLER__",
                    &BoardReset::verilog_identity().module_name(),
                )
                .replace(
                    "__BOOT_PROGRESS_MONITOR__",
                    &BootProgressMonitor::verilog_identity().module_name(),
                ),
        )
    }

    fn verilog_dependencies() -> Vec<VerilogDependency> {
        vec![
            VerilogDependency::new::<BootMemory>("u_boot"),
            VerilogDependency::new::<CpuV3Core>("u_core"),
            VerilogDependency::new::<CpuV3InstructionFetchQueue>("u_instruction_fetch_queue"),
            VerilogDependency::new::<CpuV3InstructionCache>("u_instruction_cache"),
            VerilogDependency::new::<CpuV3DataCache>("u_data_cache"),
            VerilogDependency::new::<CpuV3MemoryArbiter>("u_memory_arbiter"),
            VerilogDependency::new::<SystemControl>("u_sysctl"),
            VerilogDependency::new::<BootDmaDevice>("u_boot_dma_device"),
            VerilogDependency::new::<BootDmaEngine>("u_boot_dma_engine"),
            VerilogDependency::new::<FittedFlashReader>("u_flash"),
            VerilogDependency::new::<SharedSdramPort>("u_shared_sdram_port"),
            VerilogDependency::new::<FramebufferHdmi>("u_display"),
            VerilogDependency::new::<BoardReset>("u_reset"),
            VerilogDependency::new::<BootProgressMonitor>("u_boot_progress"),
        ]
    }

    fn verilog_testbench() -> Option<String> {
        let mut flash_init = String::new();
        for (index, byte) in FLASH_PACKAGE.iter().enumerate() {
            flash_init.push_str(&format!("        flash_image[{index}] = 8'h{byte:02x};\n"));
        }
        Some(
            include_str!("signature_testbench.v")
                .replace("__FLASH_PACKAGE_SIZE__", &FLASH_PACKAGE.len().to_string())
                .replace("__FLASH_PACKAGE_INIT__", &flash_init),
        )
    }
}

fn gowin_project() -> GowinModuleProject<TangNano20K, CpuV3System> {
    // The board video PLL follows the single display-mode switch so the fitted
    // pixel clock always matches the compiled-in scanout timing.
    let video_mode = TangNano20KVideoMode::from_pixel_clock(ACTIVE_DISPLAY_CONFIG.pixel_clock_hz);
    TangNano20K::boot_hdmi_memory_project::<CpuV3System>("cpu_v3_system", video_mode)
        .expect_bsram_blocks(ResourceCountExpectation::Claimed)
        .expect_dsp_mode(GowinDspMode::Mult18x18, ResourceCountExpectation::Claimed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cpu_v3::rcc_backend::{self, CompilerOptions, CpuV3Program};
    use cpu_v3_tang_nano_20k::boot::{
        build_boot_image, PackManifest, S1_APPLICATION_LAYOUT, S2_APPLICATION_LAYOUT,
    };
    use digital_design_hardware::{ResourceKind, VerilogProject};
    use rcc::frontend::compile_program_named;
    use std::path::Path;

    fn compile_cpu_v3(file: &str, options: &CompilerOptions) -> CpuV3Program {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("rcc")
            .join(file);
        let source = std::fs::read_to_string(&path).expect("read rcc source");
        let source_dir = path.parent().expect("rcc source directory");
        let program =
            compile_program_named(&path.display().to_string(), &source, options, &mut |name| {
                let path = if name == "boot_selection" {
                    std::path::PathBuf::from(env!("OUT_DIR")).join("boot-selection.generated.rs")
                } else {
                    source_dir.join(format!("{name}.rs"))
                };
                std::fs::read_to_string(path)
                    .map_err(|error| format!("read module `{name}`: {error}"))
            })
            .expect("rcc compile failed");
        rcc_backend::try_compile(program, options, "main")
            .unwrap_or_else(|error| panic!("cpu-v3 compile failed: {error}"))
    }

    fn format_words(words: &[u16]) -> String {
        let items = words
            .iter()
            .map(|word| format!("0x{word:04x}"))
            .collect::<Vec<_>>();
        format!("&[{}]", items.join(", "))
    }

    #[test]
    fn stage0_image_is_the_current_compiler_output() {
        let compiled = compile_cpu_v3("stage0.rs", &CompilerOptions::default()).words;
        assert!(
            compiled.len() < BSRAM_1024_DEPTH,
            "stage0 uses {} words; the boot window holds {BSRAM_1024_DEPTH}",
            compiled.len()
        );
        assert_eq!(
            compiled.len(),
            STAGE0_PROGRAM.len(),
            "stage0 changed; new STAGE0_PROGRAM = {}",
            format_words(&compiled)
        );
        if compiled != STAGE0_PROGRAM {
            panic!(
                "stage0 changed; new STAGE0_PROGRAM = {}",
                format_words(&compiled)
            );
        }
        assert_eq!(
            BootImage::WORDS[..compiled.len()],
            compiled.iter().copied().map(u64::from).collect::<Vec<_>>()
        );
    }

    #[test]
    fn generated_manifest_repackages_the_embedded_flash_bytes() {
        let manifest = PackManifest::parse(include_str!(concat!(
            env!("OUT_DIR"),
            "/boot.cpu-v3-manifest"
        )))
        .unwrap();
        let spec = manifest.load(Path::new(env!("OUT_DIR"))).unwrap();
        let package = build_boot_image(spec).unwrap().bytes;
        assert_eq!(package, FLASH_PACKAGE);
    }

    #[test]
    fn generated_manifest_contains_the_derived_two_section_layout() {
        let manifest = PackManifest::parse(include_str!(concat!(
            env!("OUT_DIR"),
            "/boot.cpu-v3-manifest"
        )))
        .unwrap();
        assert_eq!(manifest.sections.len(), 2);
        let s1_section = manifest
            .sections
            .iter()
            .find(|section| section.name == S1_APPLICATION_LAYOUT.section_name)
            .unwrap();
        let s2_section = manifest
            .sections
            .iter()
            .find(|section| section.name == S2_APPLICATION_LAYOUT.section_name)
            .unwrap();
        assert_eq!(manifest.application_entry, S1_APPLICATION_LAYOUT.entry);
        assert_eq!(s1_section.destination, S1_APPLICATION_LAYOUT.destination());
        assert_eq!(
            s1_section.source.as_deref(),
            Some(Path::new(S1_APPLICATION_LAYOUT.asset_name))
        );
        assert_eq!(s2_section.destination, S2_APPLICATION_LAYOUT.destination());
        assert_eq!(
            s2_section.source.as_deref(),
            Some(Path::new(S2_APPLICATION_LAYOUT.asset_name))
        );
    }

    #[test]
    fn project_contains_full_system_memory_flash_and_display() {
        let verilog = VerilogProject::generate::<CpuV3System>().unwrap();
        assert!(!verilog.resource_claims.is_empty());
        let project = gowin_project().generate().unwrap();
        assert_eq!(project.resources.claimed[&ResourceKind::SdrSdramDevice], 1);
        assert_eq!(project.resources.claimed[&ResourceKind::SpiFlashDevice], 1);
        assert_eq!(project.resources.claimed[&ResourceKind::Pll], 2);
        assert_eq!(project.resources.claimed[&ResourceKind::HdmiOutput], 1);
        assert_eq!(project.resources.claimed[&ResourceKind::Bsram18K], 7);
    }

    #[test]
    #[ignore = "explicit external simulator validation"]
    fn flash_boot_executes_in_verilog() {
        digital_design_hardware::verify_verilog_with_iverilog::<CpuV3System>().unwrap();
    }
}
