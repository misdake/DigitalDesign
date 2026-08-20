use digital_design_code::CircuitWires;
use digital_design_hardware::{
    run_gowin_project_cli, BootDmaMmio, Bsram1R1Rw1024, BsramImage, G16Core, G16DirectMappedCache,
    G16MemoryArbiter, G16MmioBridge, GowinCliError, GowinDspMode, GowinModuleProject, Hardware,
    HardwareIdentity, Module, ResourceCountExpectation, TangNano20K, TangNano20KSdramInputs,
    TangNano20KSdramOutputs, TangNano20KSdramWordPort, VerilogDependency, BSRAM_1024_DEPTH,
};

fn main() -> Result<(), GowinCliError> {
    run_gowin_project_cli(gowin_project(), "target/g16_sdram_gowin")
}

const PROGRAM: [u16; 14] = [
    0xfff0, 0xafd0, 0xfff0, 0xaff0, 0xf400, 0xaf00, 0xf123, 0xaf14, 0x9100, 0x8000, 0xaf11, 0x0001,
    0xe800, 0xe800,
];

struct BootImage;

const fn boot_image() -> [u64; BSRAM_1024_DEPTH] {
    let mut words = [0; BSRAM_1024_DEPTH];
    let mut index = 0;
    while index < words.len() {
        words[index] = (((index as u64) * 0x9e37) ^ 0x5aa5) & 0xffff;
        index += 1;
    }
    index = 0;
    while index < PROGRAM.len() {
        words[index] = PROGRAM[index] as u64;
        index += 1;
    }
    words
}

impl BsramImage<16> for BootImage {
    const WORDS: [u64; BSRAM_1024_DEPTH] = boot_image();
}

type BootMemory = Bsram1R1Rw1024<16, BootImage>;

#[derive(Hardware)]
#[hardware(namespace = "examples/g16_sdram")]
struct G16SdramBoardTest;

impl Module for G16SdramBoardTest {
    type Input = TangNano20KSdramInputs;
    type Output = TangNano20KSdramOutputs;
    type EmuState = ();

    const USES_MAIN_CLOCK: bool = true;
    const EMU_AVAILABLE: bool = false;

    fn execute_emu(
        _state: &mut Self::EmuState,
        _circuit: &mut CircuitWires,
        _input: &Self::Input,
        _output: &Self::Output,
    ) {
        panic!("G16SdramBoardTest is a Verilog-only hardware integration test")
    }

    fn verilog_source() -> Option<String> {
        Some(
            include_str!("self_test.v")
                .replace(
                    "__BOOT_MEMORY__",
                    &BootMemory::verilog_identity().module_name(),
                )
                .replace("__G16_CORE__", &G16Core::verilog_identity().module_name())
                .replace(
                    "__CACHE__",
                    &G16DirectMappedCache::verilog_identity().module_name(),
                )
                .replace(
                    "__ARBITER__",
                    &G16MemoryArbiter::verilog_identity().module_name(),
                )
                .replace(
                    "__MMIO_BRIDGE__",
                    &G16MmioBridge::verilog_identity().module_name(),
                )
                .replace(
                    "__BOOT_DMA_MMIO__",
                    &BootDmaMmio::verilog_identity().module_name(),
                )
                .replace(
                    "__SDRAM_WORD_PORT__",
                    &TangNano20KSdramWordPort::verilog_identity().module_name(),
                ),
        )
    }

    fn verilog_dependencies() -> Vec<VerilogDependency> {
        vec![
            VerilogDependency::new::<BootMemory>("u_boot"),
            VerilogDependency::new::<G16Core>("u_core"),
            VerilogDependency::new::<G16DirectMappedCache>("u_instruction_cache"),
            VerilogDependency::new::<G16DirectMappedCache>("u_data_cache"),
            VerilogDependency::new::<G16MemoryArbiter>("u_memory_arbiter"),
            VerilogDependency::new::<G16MmioBridge>("u_mmio_bridge"),
            VerilogDependency::new::<BootDmaMmio>("u_boot_dma_mmio"),
            VerilogDependency::new::<TangNano20KSdramWordPort>("u_sdram_word_port"),
        ]
    }

    fn verilog_testbench() -> Option<String> {
        Some(include_str!("signature_testbench.v").to_string())
    }
}

fn gowin_project() -> GowinModuleProject<TangNano20K, G16SdramBoardTest> {
    TangNano20K::sdram_debug_uart_project::<G16SdramBoardTest>("g16_sdram_self_test")
        .expect_bsram_blocks(ResourceCountExpectation::Exact(3))
        .expect_dsp_mode(GowinDspMode::Mult18x18, ResourceCountExpectation::Exact(1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cpu_v2::{frontend::parse_source_with, Compiler, CompilerOptions};
    use digital_design_hardware::{ResourceKind, VerilogProject};

    const SOURCE: &str = r#"
        static VALUE: u16 = 0;
        fn main() {
            let mut words = addr_of(&VALUE).as_u16_array();
            words[0u16] = 0x1234;
            halt(words[0u16] + 1);
        }
    "#;

    #[test]
    fn boot_line_is_the_current_g16_compiler_output() {
        let options = CompilerOptions::g16();
        let frontend = parse_source_with(SOURCE, options.data_base).unwrap();
        let mut compiler = Compiler::new();
        compiler.opts = options;
        for function in frontend.funcs {
            compiler.add_func(function);
        }
        let compiled = compiler.finish_g16("main").words;
        assert_eq!(compiled, PROGRAM);
        assert_eq!(
            BootImage::WORDS[..compiled.len()],
            compiled.iter().copied().map(u64::from).collect::<Vec<_>>()
        );
    }

    #[test]
    fn project_contains_boot_and_split_cache_bsram() {
        let verilog = VerilogProject::generate::<G16SdramBoardTest>().unwrap();
        assert_eq!(verilog.resource_claims.len(), 6);
        let project = gowin_project().generate().unwrap();
        assert_eq!(project.resources.claimed[&ResourceKind::Bsram18K], 3);
        assert_eq!(project.resources.claimed[&ResourceKind::SsramBit], 1_536);
        assert_eq!(project.resources.claimed[&ResourceKind::SdrSdramDevice], 1);
        assert_eq!(project.resources.claimed[&ResourceKind::Pll], 1);
        assert_eq!(project.resources.claimed[&ResourceKind::Multiplier18x18], 1);
    }

    #[test]
    #[ignore = "explicit external simulator validation"]
    fn boot_sdram_refill_and_cpu_execute_in_verilog() {
        digital_design_hardware::verify_verilog_with_iverilog::<G16SdramBoardTest>().unwrap();
    }
}
