use cpu_v3_tang_nano_20k::BootDmaEngine;
use digital_design_circuit::CircuitWires;
use digital_design_hardware::{Hardware, HardwareIdentity, Module, VerilogDependency};
use digital_design_hardware_common::DiagnosticReporter;
use digital_design_hardware_gowin::{
    run_gowin_project_cli, ErasedSpiFlashImage, GowinCliError, SpiFlashReader, TangNano20K,
    TangNano20KBootInputs, TangNano20KBootOutputs, TangNano20KSdramWordPort,
};

fn main() -> Result<(), GowinCliError> {
    run_gowin_project_cli(
        TangNano20K::boot_memory_project::<BootDmaSelfTest>("boot_dma_self_test"),
        "target/boot_dma_gowin",
    )
}

#[derive(Hardware)]
#[hardware(namespace = "examples/boot_dma")]
struct BootDmaSelfTest;

type BootDmaReporter = DiagnosticReporter<0x06, 469, 1, 1>;
type FittedFlashReader = SpiFlashReader<ErasedSpiFlashImage, 8_388_608, 2>;

impl Module for BootDmaSelfTest {
    type Input = TangNano20KBootInputs;
    type Output = TangNano20KBootOutputs;
    type EmuState = ();

    const USES_MAIN_CLOCK: bool = true;
    const EMU_AVAILABLE: bool = false;

    fn execute_emu(
        _state: &mut Self::EmuState,
        _circuit: &mut CircuitWires,
        _input: &Self::Input,
        _output: &Self::Output,
    ) {
        panic!("BootDmaSelfTest is a Verilog-only board test")
    }

    fn verilog_source() -> Option<String> {
        Some(
            include_str!("self_test.v")
                .replace(
                    "__BOOT_DMA_ENGINE__",
                    &BootDmaEngine::verilog_identity().module_name(),
                )
                .replace(
                    "__FLASH_READER__",
                    &FittedFlashReader::verilog_identity().module_name(),
                )
                .replace(
                    "__SDRAM_WORD_PORT__",
                    &TangNano20KSdramWordPort::verilog_identity().module_name(),
                )
                .replace(
                    "__DIAGNOSTIC_REPORTER__",
                    &BootDmaReporter::verilog_identity().module_name(),
                ),
        )
    }

    fn verilog_dependencies() -> Vec<VerilogDependency> {
        vec![
            VerilogDependency::new::<BootDmaEngine>("u_dma"),
            VerilogDependency::new::<FittedFlashReader>("u_flash"),
            VerilogDependency::new::<TangNano20KSdramWordPort>("u_memory"),
            VerilogDependency::new::<BootDmaReporter>("u_reporter"),
        ]
    }

    fn verilog_testbench() -> Option<String> {
        Some(include_str!("signature_testbench.v").to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use digital_design_hardware::{ResourceKind, VerilogProject};

    #[test]
    fn project_claims_each_fitted_memory_once() {
        let verilog = VerilogProject::generate::<BootDmaSelfTest>().unwrap();
        assert_eq!(verilog.resource_claims.len(), 1);
        let project = TangNano20K::boot_memory_project::<BootDmaSelfTest>("test")
            .generate()
            .unwrap();
        assert_eq!(project.resources.claimed[&ResourceKind::SpiFlashDevice], 1);
        assert_eq!(project.resources.claimed[&ResourceKind::SdrSdramDevice], 1);
        assert_eq!(project.resources.claimed[&ResourceKind::Pll], 1);
    }

    #[test]
    #[ignore = "explicit external simulator validation"]
    fn verify_board_harness_with_iverilog() {
        digital_design_hardware::verify_verilog_with_iverilog::<BootDmaSelfTest>().unwrap();
    }
}
