use digital_design_circuit::CircuitWires;
use digital_design_hardware_gowin::{
    run_gowin_project_cli, GowinCliError, Hardware, Module, SpiFlash, TangNano20K,
    TangNano20KFlashInputs, TangNano20KFlashOutputs, TargetResourceRequest,
};

fn main() -> Result<(), GowinCliError> {
    run_gowin_project_cli(
        gowin_project(),
        "target/cpu_v3_boot_flash_diagnostics_gowin",
    )
}

/// Non-destructive SPI NOR characterization. It reads JEDEC/status registers,
/// briefly sets the volatile write-enable latch, reads it back, then clears it.
#[derive(Hardware)]
#[hardware(
    namespace = "systems/cpu_v3_tang_nano_20k/boot_flash_diagnostics",
    target_leaf
)]
struct FlashDiagnosticsProbe;

impl Module for FlashDiagnosticsProbe {
    type Input = TangNano20KFlashInputs;
    type Output = TangNano20KFlashOutputs;
    type EmuState = ();

    const USES_MAIN_CLOCK: bool = true;
    const EMU_AVAILABLE: bool = false;

    fn target_resources() -> Vec<TargetResourceRequest> {
        vec![TargetResourceRequest::new(SpiFlash)]
    }

    fn execute_emu(
        _state: &mut Self::EmuState,
        _circuit: &mut CircuitWires,
        _input: &Self::Input,
        _output: &Self::Output,
    ) {
        panic!("FlashDiagnosticsProbe is a Verilog-only hardware probe")
    }

    fn verilog_source() -> Option<String> {
        Some(include_str!("self_test.v").to_string())
    }

    fn verilog_testbench() -> Option<String> {
        Some(include_str!("probe_testbench.v").to_string())
    }
}

fn gowin_project(
) -> digital_design_hardware_gowin::GowinModuleProject<TangNano20K, FlashDiagnosticsProbe> {
    TangNano20K::flash_debug_uart_project::<FlashDiagnosticsProbe>("flash_diagnostics_probe")
}

#[cfg(test)]
mod tests {
    use super::*;
    use digital_design_hardware::VerilogProject;

    #[test]
    fn project_owns_exactly_one_fitted_flash() {
        let verilog = VerilogProject::generate::<FlashDiagnosticsProbe>().unwrap();
        assert_eq!(verilog.resource_claims.len(), 1);
        let project = gowin_project().generate().unwrap();
        assert_eq!(
            project.resources.claimed[&digital_design_hardware::ResourceKind::SpiFlashDevice],
            1
        );
    }

    #[test]
    #[ignore = "explicit external simulator validation"]
    fn diagnostic_record_decodes_in_verilog() {
        digital_design_hardware::verify_verilog_with_iverilog::<FlashDiagnosticsProbe>().unwrap();
    }
}
