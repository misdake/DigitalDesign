use cpu_v3::CpuV3Core;
use digital_design_circuit::CircuitWires;
use digital_design_hardware::{Hardware, HardwareIdentity, Module, VerilogDependency};
use digital_design_hardware_common::{DiagnosticReporter, ResetController};
use digital_design_hardware_gowin::{
    run_gowin_project_cli, Bsram1R1Rw1024, BsramImage, GowinCliError, GowinDspMode,
    GowinModuleProject, ResourceCountExpectation, TangNano20K, TangNano20KDebugOutputs,
    TangNano20KInputs, BSRAM_1024_DEPTH,
};

fn main() -> Result<(), GowinCliError> {
    run_gowin_project_cli(gowin_project(), "target/cpu_v3_cpu_gowin")
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
type BoardReset = ResetController<8>;
type CpuReporter = DiagnosticReporter<0x04, 234, 13_500_000, 13_500_000>;

#[derive(Hardware)]
#[hardware(namespace = "examples/cpu_v3_cpu")]
struct CpuV3CpuBoardTest;

impl Module for CpuV3CpuBoardTest {
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
        panic!("CpuV3CpuBoardTest is a Verilog-only hardware test harness")
    }

    fn verilog_source() -> Option<String> {
        Some(
            include_str!("self_test.v")
                .replace(
                    "__PROGRAM_MEMORY__",
                    &ProgramMemory::verilog_identity().module_name(),
                )
                .replace(
                    "__CPU_V3_CORE__",
                    &CpuV3Core::verilog_identity().module_name(),
                )
                .replace(
                    "__RESET_CONTROLLER__",
                    &BoardReset::verilog_identity().module_name(),
                )
                .replace(
                    "__DIAGNOSTIC_REPORTER__",
                    &CpuReporter::verilog_identity().module_name(),
                ),
        )
    }

    fn verilog_dependencies() -> Vec<VerilogDependency> {
        vec![
            VerilogDependency::new::<ProgramMemory>("u_program"),
            VerilogDependency::new::<CpuV3Core>("u_core"),
            VerilogDependency::new::<BoardReset>("u_reset"),
            VerilogDependency::new::<CpuReporter>("u_reporter"),
        ]
    }

    fn verilog_testbench() -> Option<String> {
        Some(include_str!("signature_testbench.v").to_string())
    }
}

fn gowin_project() -> GowinModuleProject<TangNano20K, CpuV3CpuBoardTest> {
    TangNano20K::debug_uart_project::<CpuV3CpuBoardTest>("cpu_v3_cpu_self_test")
        .expect_dsp_mode(GowinDspMode::Mult18x18, ResourceCountExpectation::Exact(1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cpu_v3::rcc_backend::{self, CompilerOptions};
    use digital_design_hardware::{ResourceKind, VerilogProject};
    use rcc::frontend::parse_source_with;

    const SOURCE: &str = r#"
        fn main() {
            let mut sum: u16 = 0;
            let mut i: u16 = 5;
            while i != 0 { sum = sum + i; i = i - 1; }
            halt(sum);
        }
    "#;

    #[test]
    fn boot_image_is_the_current_cpu_v3_compiler_output() {
        let options = CompilerOptions::default();
        let frontend = parse_source_with(SOURCE, options.data_base).unwrap();
        let compiled = rcc_backend::compile(frontend, &options, "main").words;
        assert_eq!(compiled, PROGRAM);
        assert_eq!(
            ProgramImage::WORDS[..compiled.len()],
            compiled.iter().copied().map(u64::from).collect::<Vec<_>>()
        );
    }

    #[test]
    fn project_contains_program_bsram_and_reusable_core() {
        let verilog = VerilogProject::generate::<CpuV3CpuBoardTest>().unwrap();
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
        digital_design_hardware::verify_verilog_with_iverilog::<CpuV3CpuBoardTest>().unwrap();
    }
}
