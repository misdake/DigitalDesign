use cpu_v3::{CpuV3Core, CpuV3DirectMappedCache};
use cpu_v3_tang_nano_20k::{BootDmaDevice, CpuV3MemoryArbiter};
use digital_design_circuit::CircuitWires;
use digital_design_hardware::{Hardware, HardwareIdentity, Module, VerilogDependency};
use digital_design_hardware_common::{DiagnosticReporter, ResetController};
use digital_design_hardware_gowin::{
    run_gowin_project_cli, Bsram1R1Rw1024, BsramImage, GowinCliError, GowinDspMode,
    GowinModuleProject, ResourceCountExpectation, TangNano20K, TangNano20KSdramInputs,
    TangNano20KSdramOutputs, TangNano20KSdramWordPort, BSRAM_1024_DEPTH,
};

include!(concat!(env!("OUT_DIR"), "/sdram_self_test_image.rs"));

fn main() -> Result<(), GowinCliError> {
    run_gowin_project_cli(gowin_project(), "target/cpu_v3_sdram_gowin")
}

struct BootImage;

const fn boot_image() -> [u64; BSRAM_1024_DEPTH] {
    let mut words = [0; BSRAM_1024_DEPTH];
    let mut index = 0;
    while index < words.len() {
        words[index] = (((index as u64) * 0x9e37) ^ 0x5aa5) & 0xffff;
        index += 1;
    }
    index = 0;
    while index < SDRAM_SELF_TEST_PROGRAM.len() {
        words[index] = SDRAM_SELF_TEST_PROGRAM[index] as u64;
        index += 1;
    }
    words
}

impl BsramImage<16> for BootImage {
    const WORDS: [u64; BSRAM_1024_DEPTH] = boot_image();
}

type BootMemory = Bsram1R1Rw1024<16, BootImage>;
type BoardReset = ResetController<8>;
type SdramCpuReporter = DiagnosticReporter<0x05, 469, 13_500_000, 13_500_000>;

#[derive(Hardware)]
#[hardware(namespace = "examples/cpu_v3_sdram")]
struct CpuV3SdramBoardTest;

impl Module for CpuV3SdramBoardTest {
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
        panic!("CpuV3SdramBoardTest is a Verilog-only hardware integration test")
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
                    "__CACHE__",
                    &CpuV3DirectMappedCache::verilog_identity().module_name(),
                )
                .replace(
                    "__ARBITER__",
                    &CpuV3MemoryArbiter::verilog_identity().module_name(),
                )
                .replace(
                    "__BOOT_DMA_DEVICE__",
                    &BootDmaDevice::verilog_identity().module_name(),
                )
                .replace(
                    "__SDRAM_WORD_PORT__",
                    &TangNano20KSdramWordPort::verilog_identity().module_name(),
                )
                .replace(
                    "__RESET_CONTROLLER__",
                    &BoardReset::verilog_identity().module_name(),
                )
                .replace(
                    "__DIAGNOSTIC_REPORTER__",
                    &SdramCpuReporter::verilog_identity().module_name(),
                ),
        )
    }

    fn verilog_dependencies() -> Vec<VerilogDependency> {
        vec![
            VerilogDependency::new::<BootMemory>("u_boot"),
            VerilogDependency::new::<CpuV3Core>("u_core"),
            VerilogDependency::new::<CpuV3DirectMappedCache>("u_instruction_cache"),
            VerilogDependency::new::<CpuV3DirectMappedCache>("u_data_cache"),
            VerilogDependency::new::<CpuV3MemoryArbiter>("u_memory_arbiter"),
            VerilogDependency::new::<BootDmaDevice>("u_boot_dma_device"),
            VerilogDependency::new::<TangNano20KSdramWordPort>("u_sdram_word_port"),
            VerilogDependency::new::<BoardReset>("u_reset"),
            VerilogDependency::new::<SdramCpuReporter>("u_reporter"),
        ]
    }

    fn verilog_testbench() -> Option<String> {
        Some(include_str!("signature_testbench.v").to_string())
    }
}

fn gowin_project() -> GowinModuleProject<TangNano20K, CpuV3SdramBoardTest> {
    TangNano20K::sdram_debug_uart_project::<CpuV3SdramBoardTest>("cpu_v3_sdram_self_test")
        .expect_bsram_blocks(ResourceCountExpectation::Claimed)
        .expect_dsp_mode(GowinDspMode::Mult18x18, ResourceCountExpectation::Claimed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cpu_v3::{Machine, RunOutcome};
    use digital_design_hardware::{ResourceKind, VerilogProject};

    #[test]
    fn generated_boot_line_executes_in_the_cpu_v3_oracle() {
        let mut machine = Machine::default();
        machine.load_program(0, SDRAM_SELF_TEST_PROGRAM).unwrap();
        assert!(matches!(
            machine.run(1_000).unwrap(),
            RunOutcome::Halted { signal: 0x1235, .. }
        ));
        assert_eq!(
            BootImage::WORDS[..SDRAM_SELF_TEST_PROGRAM.len()],
            SDRAM_SELF_TEST_PROGRAM
                .iter()
                .copied()
                .map(u64::from)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn project_contains_boot_and_split_cache_bsram() {
        let verilog = VerilogProject::generate::<CpuV3SdramBoardTest>().unwrap();
        assert!(!verilog.resource_claims.is_empty());
        let project = gowin_project().generate().unwrap();
        assert_eq!(project.resources.claimed[&ResourceKind::SdrSdramDevice], 1);
        assert_eq!(project.resources.claimed[&ResourceKind::Pll], 1);
        assert_eq!(project.resources.claimed[&ResourceKind::Bsram18K], 6);
    }

    #[test]
    #[ignore = "explicit external simulator validation"]
    fn boot_sdram_refill_and_cpu_execute_in_verilog() {
        digital_design_hardware::verify_verilog_with_iverilog::<CpuV3SdramBoardTest>().unwrap();
    }
}
