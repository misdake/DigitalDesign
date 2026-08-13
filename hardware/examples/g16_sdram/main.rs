use digital_design_code::CircuitWires;
use digital_design_hardware::{
    run_gowin_project_cli, Bsram1R1Rw1024, BsramImage, GowinCliError, GowinModuleProject, Hardware,
    HardwareIdentity, Module, TangNano20K, TangNano20KSdramInputs, TangNano20KSdramOutputs,
    VerilogDependency, ZeroBsramImage, BSRAM_1024_DEPTH,
};

fn main() -> Result<(), GowinCliError> {
    run_gowin_project_cli(gowin_project(), "target/g16_sdram_gowin")
}

const PROGRAM: [u16; 17] = [
    0xfff0, 0xafd0, 0xfff0, 0xaff0, 0xaf00, 0xaf15, 0xe1c1, 0xa9c0, 0xf000, 0xb0c1, 0xe800, 0x0001,
    0xaf21, 0x1112, 0xf0ff, 0xcff6, 0xe800,
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
type InstructionCacheMemory = Bsram1R1Rw1024<16, ZeroBsramImage>;

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
                .replace(
                    "__INSTRUCTION_CACHE__",
                    &InstructionCacheMemory::verilog_identity().module_name(),
                ),
        )
    }

    fn verilog_dependencies() -> Vec<VerilogDependency> {
        vec![
            VerilogDependency::new::<BootMemory>("u_boot"),
            VerilogDependency::new::<InstructionCacheMemory>("u_instruction_cache"),
        ]
    }

    fn verilog_testbench() -> Option<String> {
        Some(include_str!("signature_testbench.v").to_string())
    }
}

fn gowin_project() -> GowinModuleProject<TangNano20K, G16SdramBoardTest> {
    TangNano20K::sdram_debug_uart_project::<G16SdramBoardTest>("g16_sdram_self_test")
}

#[cfg(test)]
mod tests {
    use super::*;
    use cpu_v2::{frontend::parse_source_with, Compiler, CompilerOptions};
    use digital_design_hardware::{ResourceKind, VerilogProject};

    const SOURCE: &str = r#"
        fn main() {
            let mut sum: u16 = 0;
            let mut i: u16 = 5;
            while i != 0 { sum = sum + i; i = i - 1; }
            halt(sum);
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
        assert!(compiled.len() > 16);
        assert_eq!(
            BootImage::WORDS[..16],
            compiled[..16]
                .iter()
                .copied()
                .map(u64::from)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn project_contains_boot_and_instruction_cache_bsram() {
        let verilog = VerilogProject::generate::<G16SdramBoardTest>().unwrap();
        assert_eq!(verilog.resource_claims.len(), 2);
        let project = gowin_project().generate().unwrap();
        assert_eq!(project.resources.claimed[&ResourceKind::Bsram18K], 2);
        assert_eq!(project.resources.claimed[&ResourceKind::SdrSdramDevice], 1);
        assert_eq!(project.resources.claimed[&ResourceKind::Pll], 1);
    }

    #[test]
    #[ignore = "explicit external simulator validation"]
    fn boot_sdram_refill_and_cpu_execute_in_verilog() {
        digital_design_hardware::verify_verilog_with_iverilog::<G16SdramBoardTest>().unwrap();
    }
}
