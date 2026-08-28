use cpu_v3::CpuV3Core;
use digital_design_circuit::CircuitWires;
use digital_design_hardware::{Hardware, HardwareIdentity, Module, VerilogDependency};
use digital_design_hardware_common::{DiagnosticReporter, ResetController};
use digital_design_hardware_gowin::{
    run_gowin_project_cli, Bsram1R1Rw1024, BsramImage, GowinCliError, GowinDspMode,
    GowinModuleProject, ResourceCountExpectation, TangNano20K, TangNano20KDebugOutputs,
    TangNano20KInputs, BSRAM_1024_DEPTH,
};

include!(concat!(env!("OUT_DIR"), "/cpu_self_test_image.rs"));

fn main() -> Result<(), GowinCliError> {
    run_gowin_project_cli(gowin_project(), "target/cpu_v3_cpu_gowin")
}

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
    while index < CPU_SELF_TEST_PROGRAM.len() {
        words[index] = CPU_SELF_TEST_PROGRAM[index] as u64;
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
        .expect_dsp_mode(GowinDspMode::Mult18x18, ResourceCountExpectation::Exact(2))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cpu_v3::{Machine, RunOutcome};
    use digital_design_hardware::{ResourceKind, VerilogProject};

    #[test]
    fn generated_boot_image_executes_in_the_cpu_v3_oracle() {
        let mut machine = Machine::default();
        machine.load_program(0, CPU_SELF_TEST_PROGRAM).unwrap();
        assert!(matches!(
            machine.run(1_000).unwrap(),
            RunOutcome::Halted { signal: 15, .. }
        ));
        assert_eq!(
            ProgramImage::WORDS[..CPU_SELF_TEST_PROGRAM.len()],
            CPU_SELF_TEST_PROGRAM
                .iter()
                .copied()
                .map(u64::from)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn project_contains_program_bsram_and_reusable_core() {
        let verilog = VerilogProject::generate::<CpuV3CpuBoardTest>().unwrap();
        assert_eq!(verilog.resource_claims.len(), 5);
        assert!(verilog
            .files
            .values()
            .any(|source| source.contains(&format!(
                "{} u_program",
                ProgramMemory::verilog_identity().module_name()
            ))));
        let project = gowin_project().generate().unwrap();
        assert_eq!(project.resources.claimed[&ResourceKind::Bsram18K], 3);
        assert_eq!(project.resources.claimed[&ResourceKind::Multiplier18x18], 2);
    }

    #[test]
    #[ignore = "explicit external simulator validation"]
    fn compiled_program_executes_in_verilog() {
        digital_design_hardware::verify_verilog_with_iverilog::<CpuV3CpuBoardTest>().unwrap();
    }
}
