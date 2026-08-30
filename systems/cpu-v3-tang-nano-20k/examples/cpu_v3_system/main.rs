use cpu_v3::{CpuV3Core, CpuV3DataCache, CpuV3InstructionCache, CpuV3InstructionFetchQueue};
use cpu_v3_tang_nano_20k::{
    BootDmaDevice, BootDmaEngine, BootProgressMonitor, CpuV3MemoryArbiter, DisplaySdramPort,
    FramebufferHdmi, SystemControlDevice,
};
use digital_design_circuit::CircuitWires;
use digital_design_hardware::{Hardware, HardwareIdentity, Module, VerilogDependency};
use digital_design_hardware_common::ResetController;
use digital_design_hardware_gowin::{
    run_gowin_project_cli, Bsram1R1Rw1024, BsramImage, ErasedSpiFlashImage, GowinCliError,
    GowinDspMode, GowinModuleProject, ResourceCountExpectation, SpiFlashReader, TangNano20K,
    TangNano20KBootHdmiInputs, TangNano20KBootHdmiOutputs, BSRAM_1024_DEPTH,
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
    type Input = TangNano20KBootHdmiInputs;
    type Output = TangNano20KBootHdmiOutputs;
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
                    "__DISPLAY_SDRAM_PORT__",
                    &DisplaySdramPort::verilog_identity().module_name(),
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
            VerilogDependency::new::<DisplaySdramPort>("u_sdram_word_port"),
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
    TangNano20K::boot_hdmi_memory_project::<CpuV3System>("cpu_v3_system")
        .expect_bsram_blocks(ResourceCountExpectation::Claimed)
        .expect_dsp_mode(GowinDspMode::Mult18x18, ResourceCountExpectation::Claimed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cpu_v3::rcc_backend::{self, CompilerOptions, CpuV3Program};
    use cpu_v3::PhysicalWordAddress;
    use cpu_v3_tang_nano_20k::boot::{
        build_boot_image, BootEntry, BootImageSpec, BootTarget, InputSection, SectionKind,
        SECTION_EXECUTE, SECTION_READ, SECTION_WRITE,
    };
    use digital_design_hardware::{ResourceKind, VerilogProject};
    use rcc::frontend::compile_program_named;

    fn compile_cpu_v3(file: &str, options: &CompilerOptions) -> CpuV3Program {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("rcc")
            .join(file);
        let source = std::fs::read_to_string(&path).expect("read rcc source");
        let source_dir = path.parent().expect("rcc source directory");
        let program =
            compile_program_named(&path.display().to_string(), &source, options, &mut |name| {
                std::fs::read_to_string(source_dir.join(format!("{name}.rs")))
                    .map_err(|error| format!("read module `{name}`: {error}"))
            })
            .expect("rcc compile failed");
        rcc_backend::compile(program, options, "main")
    }

    fn words_bytes(words: &[u16]) -> Vec<u8> {
        words.iter().flat_map(|word| word.to_le_bytes()).collect()
    }

    fn section(
        name: &str,
        destination: u32,
        data: Vec<u8>,
        memory_size_bytes: u32,
        flags: u16,
    ) -> InputSection {
        InputSection {
            name: name.into(),
            kind: SectionKind::Load,
            flags,
            destination: PhysicalWordAddress::new(destination),
            data,
            memory_size_bytes,
            alignment_bytes: 32,
        }
    }

    /// Packs Stage1 and the demo application into the Flash boot package,
    /// mirroring the section layout of the `cpu_v2` `cpu_v3_boot` test.
    fn boot_package() -> Vec<u8> {
        let stage1 = compile_cpu_v3(
            "stage1.rs",
            &CompilerOptions {
                code_base: 0x0100,
                stack_init: 0xf000,
                ..CompilerOptions::default()
            },
        );
        let application = compile_cpu_v3(
            "boot-demo.rs",
            &CompilerOptions {
                code_base: 0x0200,
                stack_init: 0xe000,
                ..CompilerOptions::default()
            },
        );
        let alternate_application = compile_cpu_v3(
            "boot-alt.rs",
            &CompilerOptions {
                code_base: 0x0200,
                stack_init: 0xe000,
                ..CompilerOptions::default()
            },
        );
        let stage1_bytes = words_bytes(&stage1.words);
        let application_bytes = words_bytes(&application.words);
        let alternate_application_bytes = words_bytes(&alternate_application.words);
        build_boot_image(BootImageSpec {
            target: BootTarget::TangNano20K,
            stage1_section: "stage1".into(),
            stage1_entry: BootEntry {
                code_segment: 1,
                offset: 0x0100,
                data_segment: 2,
                stack_offset: 0xf000,
            },
            application_entry: BootEntry {
                code_segment: 3,
                offset: 0x0200,
                data_segment: 4,
                stack_offset: 0xe000,
            },
            sections: vec![
                InputSection {
                    name: "stage1".into(),
                    kind: SectionKind::Load,
                    flags: SECTION_READ | SECTION_EXECUTE,
                    destination: PhysicalWordAddress::new(0x0001_0100),
                    memory_size_bytes: stage1_bytes.len() as u32,
                    data: stage1_bytes,
                    alignment_bytes: 32,
                },
                InputSection {
                    name: "application".into(),
                    kind: SectionKind::Load,
                    flags: SECTION_READ | SECTION_EXECUTE,
                    destination: PhysicalWordAddress::new(0x0003_0200),
                    memory_size_bytes: application_bytes.len() as u32,
                    data: application_bytes,
                    alignment_bytes: 32,
                },
                InputSection {
                    name: "application-alt".into(),
                    kind: SectionKind::Load,
                    flags: SECTION_READ | SECTION_EXECUTE,
                    destination: PhysicalWordAddress::new(0x0005_0200),
                    memory_size_bytes: alternate_application_bytes.len() as u32,
                    data: alternate_application_bytes,
                    alignment_bytes: 32,
                },
                section(
                    "data",
                    0x0004_0000,
                    vec![0xef, 0xbe, 0x55],
                    8,
                    SECTION_READ | SECTION_WRITE,
                ),
                InputSection {
                    name: "bss".into(),
                    kind: SectionKind::Zero,
                    flags: SECTION_READ | SECTION_WRITE,
                    destination: PhysicalWordAddress::new(0x0004_0100),
                    data: vec![],
                    memory_size_bytes: 64,
                    alignment_bytes: 32,
                },
            ],
        })
        .expect("boot image builds")
        .bytes
    }

    fn format_words(words: &[u16]) -> String {
        let items = words
            .iter()
            .map(|word| format!("0x{word:04x}"))
            .collect::<Vec<_>>();
        format!("&[{}]", items.join(", "))
    }

    fn format_bytes(bytes: &[u8]) -> String {
        let items = bytes
            .iter()
            .map(|byte| format!("0x{byte:02x}"))
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
    fn flash_package_is_the_current_compiler_output() {
        let package = boot_package();
        if package != FLASH_PACKAGE {
            panic!(
                "boot package changed; new FLASH_PACKAGE = {}",
                format_bytes(&package)
            );
        }
    }

    #[test]
    fn example_manifest_tracks_current_compiler_outputs() {
        let stage1 = compile_cpu_v3(
            "stage1.rs",
            &CompilerOptions {
                code_base: 0x0100,
                stack_init: 0xf000,
                ..CompilerOptions::default()
            },
        );
        let application = compile_cpu_v3(
            "boot-demo.rs",
            &CompilerOptions {
                code_base: 0x0200,
                stack_init: 0xe000,
                ..CompilerOptions::default()
            },
        );
        let alternate_application = compile_cpu_v3(
            "boot-alt.rs",
            &CompilerOptions {
                code_base: 0x0200,
                stack_init: 0xe000,
                ..CompilerOptions::default()
            },
        );
        let manifest =
            cpu_v3_tang_nano_20k::boot::PackManifest::parse(include_str!("boot.cpu-v3-manifest"))
                .unwrap();
        let stage1_section = manifest
            .sections
            .iter()
            .find(|section| section.name == "stage1")
            .unwrap();
        let application_section = manifest
            .sections
            .iter()
            .find(|section| section.name == "application")
            .unwrap();
        let alternate_application_section = manifest
            .sections
            .iter()
            .find(|section| section.name == "application-alt")
            .unwrap();
        assert_eq!(
            stage1_section.memory_size_bytes,
            stage1.words.len() as u32 * 2
        );
        assert_eq!(
            application_section.memory_size_bytes,
            application.words.len() as u32 * 2
        );
        assert_eq!(
            alternate_application_section.memory_size_bytes,
            alternate_application.words.len() as u32 * 2
        );
        assert_eq!(
            stage1_section.source.as_deref(),
            Some(std::path::Path::new(
                "../../../../target/cpu-v3-boot/stage1.v3bin"
            ))
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
    fn two_stage_flash_boot_executes_in_verilog() {
        digital_design_hardware::verify_verilog_with_iverilog::<CpuV3System>().unwrap();
    }
}
