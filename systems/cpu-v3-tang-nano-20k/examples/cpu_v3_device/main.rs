use cpu_v3::CpuV3Core;
use cpu_v3_tang_nano_20k::SystemControlDevice;
use digital_design_circuit::CircuitWires;
use digital_design_hardware::{Hardware, HardwareIdentity, Module, VerilogDependency};
use digital_design_hardware_common::ResetController;
use digital_design_hardware_gowin::{
    run_gowin_project_cli, Bsram1R1Rw1024, BsramImage, GowinCliError, GowinDspMode,
    GowinModuleProject, ResourceCountExpectation, TangNano20K, TangNano20KDebugOutputs,
    TangNano20KInputs, BSRAM_1024_DEPTH,
};

include!(concat!(env!("OUT_DIR"), "/device_diagnostic_image.rs"));

fn main() -> Result<(), GowinCliError> {
    run_gowin_project_cli(gowin_project(), "target/cpu_v3_device_gowin")
}

struct ProgramImage;

const fn program_image() -> [u64; BSRAM_1024_DEPTH] {
    let mut words = [0; BSRAM_1024_DEPTH];
    let mut index = 0;
    // Keep the complete boot memory materialized as BSRAM (see cpu_v3_cpu).
    while index < words.len() {
        words[index] = (((index as u64) * 0x9e37) ^ 0x5aa5) & 0xffff;
        index += 1;
    }
    index = 0;
    while index < DEVICE_DIAGNOSTIC_PROGRAM.len() {
        words[index] = DEVICE_DIAGNOSTIC_PROGRAM[index] as u64;
        index += 1;
    }
    words
}

impl BsramImage<16> for ProgramImage {
    const WORDS: [u64; BSRAM_1024_DEPTH] = program_image();
}

type ProgramMemory = Bsram1R1Rw1024<16, ProgramImage>;
type SystemControl = SystemControlDevice<234>;
type BoardReset = ResetController<8>;

#[derive(Hardware)]
#[hardware(namespace = "examples/cpu_v3_device")]
struct CpuV3DeviceBoardTest;

impl Module for CpuV3DeviceBoardTest {
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
        panic!("CpuV3DeviceBoardTest is a Verilog-only hardware test harness")
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
                ),
        )
    }

    fn verilog_dependencies() -> Vec<VerilogDependency> {
        vec![
            VerilogDependency::new::<ProgramMemory>("u_program"),
            VerilogDependency::new::<CpuV3Core>("u_core"),
            VerilogDependency::new::<SystemControl>("u_sysctl"),
            VerilogDependency::new::<BoardReset>("u_reset"),
        ]
    }

    fn verilog_testbench() -> Option<String> {
        Some(include_str!("signature_testbench.v").to_string())
    }
}

fn gowin_project() -> GowinModuleProject<TangNano20K, CpuV3DeviceBoardTest> {
    TangNano20K::debug_uart_project::<CpuV3DeviceBoardTest>("cpu_v3_device_self_test")
        .expect_dsp_mode(GowinDspMode::Mult18x18, ResourceCountExpectation::Exact(2))
}

#[cfg(test)]
mod tests {
    use super::*;
    use digital_design_hardware::{ResourceKind, VerilogProject};

    #[test]
    fn generated_program_fits_the_boot_memory() {
        assert!(
            DEVICE_DIAGNOSTIC_PROGRAM.len() < BSRAM_1024_DEPTH,
            "program uses {} words; the boot memory holds {BSRAM_1024_DEPTH}",
            DEVICE_DIAGNOSTIC_PROGRAM.len()
        );
        assert_eq!(
            ProgramImage::WORDS[..DEVICE_DIAGNOSTIC_PROGRAM.len()],
            DEVICE_DIAGNOSTIC_PROGRAM
                .iter()
                .copied()
                .map(u64::from)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn project_contains_program_memory_core_and_sysctl() {
        let verilog = VerilogProject::generate::<CpuV3DeviceBoardTest>().unwrap();
        assert_eq!(verilog.resource_claims.len(), 5);
        let project = gowin_project().generate().unwrap();
        assert_eq!(project.resources.claimed[&ResourceKind::Bsram18K], 3);
        assert_eq!(project.resources.claimed[&ResourceKind::Multiplier18x18], 2);
    }

    #[test]
    #[ignore = "explicit external simulator validation"]
    fn device_diagnostic_executes_in_verilog() {
        digital_design_hardware::verify_verilog_with_iverilog::<CpuV3DeviceBoardTest>().unwrap();
    }
}
