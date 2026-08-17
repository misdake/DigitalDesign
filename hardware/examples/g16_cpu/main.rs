use digital_design_code::CircuitWires;
use digital_design_hardware::{
    run_gowin_project_cli, Bsram1R1Rw1024, BsramImage, G16Core, GowinCliError, GowinDspMode,
    GowinModuleProject, Hardware, HardwareIdentity, Module, ResourceCountExpectation, TangNano20K,
    TangNano20KDebugOutputs, TangNano20KInputs, VerilogDependency, BSRAM_1024_DEPTH,
};

fn main() -> Result<(), GowinCliError> {
    run_gowin_project_cli(gowin_project(), "target/g16_cpu_gowin")
}

const PROGRAM: [u16; 17] = [
    0xfff0, 0xafd0, 0xfff0, 0xaff0, 0xaf00, 0xaf15, 0xe1c1, 0xa9c0, 0xf000, 0xb0c1, 0xe800, 0x0001,
    0xaf21, 0x1112, 0xf0ff, 0xcff6, 0xe800,
];

struct ProgramImage;

const fn program_image() -> [u64; BSRAM_1024_DEPTH] {
    let mut words = [0; BSRAM_1024_DEPTH];
    let mut index = 0;
    // Keep the complete boot memory materialized as BSRAM. A sparse 17-word
    // image is correctly optimized into LUTs by Gowin and would not validate
    // the synchronous BSRAM fetch boundary exercised by this example.
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

impl BsramImage<16> for ProgramImage {
    const WORDS: [u64; BSRAM_1024_DEPTH] = program_image();
}

type ProgramMemory = Bsram1R1Rw1024<16, ProgramImage>;

#[derive(Hardware)]
#[hardware(namespace = "examples/g16_cpu")]
struct G16CpuBoardTest;

impl Module for G16CpuBoardTest {
    type Input = TangNano20KInputs;
    type Output = TangNano20KDebugOutputs;
    type EmuState = ();

    const USES_MAIN_CLOCK: bool = true;
    const EMU_AVAILABLE: bool = false;

    fn execute_emu(
        _state: &mut Self::EmuState,
        _circuit: &mut CircuitWires,
        _input: &Self::Input,
        _output: &Self::Output,
    ) {
        panic!("G16CpuBoardTest is a Verilog-only hardware test harness")
    }

    fn verilog_source() -> Option<String> {
        Some(
            include_str!("self_test.v")
                .replace(
                    "__PROGRAM_MEMORY__",
                    &ProgramMemory::verilog_identity().module_name(),
                )
                .replace("__G16_CORE__", &G16Core::verilog_identity().module_name()),
        )
    }

    fn verilog_dependencies() -> Vec<VerilogDependency> {
        vec![
            VerilogDependency::new::<ProgramMemory>("u_program"),
            VerilogDependency::new::<G16Core>("u_core"),
        ]
    }

    fn verilog_testbench() -> Option<String> {
        Some(include_str!("signature_testbench.v").to_string())
    }
}

fn gowin_project() -> GowinModuleProject<TangNano20K, G16CpuBoardTest> {
    TangNano20K::debug_uart_project::<G16CpuBoardTest>("g16_cpu_self_test")
        .expect_dsp_mode(GowinDspMode::Mult18x18, ResourceCountExpectation::Exact(1))
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
    fn boot_image_is_the_current_g16_compiler_output() {
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
            ProgramImage::WORDS[..compiled.len()],
            compiled.iter().copied().map(u64::from).collect::<Vec<_>>()
        );
    }

    #[test]
    fn project_contains_program_bsram_and_reusable_core() {
        let verilog = VerilogProject::generate::<G16CpuBoardTest>().unwrap();
        assert_eq!(verilog.resource_claims.len(), 2);
        assert!(verilog
            .files
            .values()
            .any(|source| source.contains(&format!(
                "{} u_program",
                ProgramMemory::verilog_identity().module_name()
            ))));
        let project = gowin_project().generate().unwrap();
        assert_eq!(project.resources.claimed[&ResourceKind::Bsram18K], 1);
        assert_eq!(project.resources.claimed[&ResourceKind::Multiplier18x18], 1);
    }

    #[test]
    #[ignore = "explicit external simulator validation"]
    fn compiled_program_executes_in_verilog() {
        digital_design_hardware::verify_verilog_with_iverilog::<G16CpuBoardTest>().unwrap();
    }
}
