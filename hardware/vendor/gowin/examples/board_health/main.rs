use digital_design_circuit::CircuitWires;
use digital_design_hardware_gowin::{
    run_gowin_project_cli, DiagnosticReporter, GowinCliError, GowinModuleProject, Hardware,
    HardwareIdentity, Module, TangNano20K, TangNano20KDebugOutputs, TangNano20KInputs,
    VerilogDependency,
};

type HealthReporter = DiagnosticReporter<0x0a, 234, 1_000_000, 5_000_000>;

fn main() -> Result<(), GowinCliError> {
    run_gowin_project_cli(gowin_project(), "target/board_health_gowin")
}

/// Minimal Tang Nano 20K transport probe. It deliberately has no dependency
/// on a CPU, fitted memory, PLL, or system device so it can gate every higher
/// level board test.
#[derive(Hardware)]
#[hardware(namespace = "examples/board_health")]
struct BoardHealthProbe;

impl Module for BoardHealthProbe {
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
        panic!("BoardHealthProbe is a Verilog-only hardware probe")
    }

    fn verilog_source() -> Option<String> {
        Some(include_str!("self_test.v").replace(
            "__DIAGNOSTIC_REPORTER__",
            &HealthReporter::verilog_identity().module_name(),
        ))
    }

    fn verilog_dependencies() -> Vec<VerilogDependency> {
        vec![VerilogDependency::new::<HealthReporter>("u_reporter")]
    }

    fn verilog_testbench() -> Option<String> {
        Some(include_str!("signature_testbench.v").to_string())
    }
}

fn gowin_project() -> GowinModuleProject<TangNano20K, BoardHealthProbe> {
    TangNano20K::debug_uart_project::<BoardHealthProbe>("board_health_probe")
}

#[cfg(test)]
mod tests {
    use super::*;
    use digital_design_hardware::{ResourceKind, VerilogProject};

    #[test]
    fn project_is_a_resource_free_board_transport_probe() {
        let verilog = VerilogProject::generate::<BoardHealthProbe>().unwrap();
        assert!(verilog.resource_claims.is_empty());
        assert!(verilog
            .files
            .values()
            .any(|source| source.contains(&format!(
                "{} u_reporter",
                HealthReporter::verilog_identity().module_name()
            ))));
        let project = gowin_project().generate().unwrap();
        for kind in [
            ResourceKind::Bsram18K,
            ResourceKind::Multiplier18x18,
            ResourceKind::Pll,
            ResourceKind::SdrSdramDevice,
            ResourceKind::SpiFlashDevice,
        ] {
            assert_eq!(
                project.resources.claimed.get(&kind).copied().unwrap_or(0),
                0
            );
        }
    }

    #[test]
    #[ignore = "explicit external simulator validation"]
    fn health_frames_and_button_diagnostics_decode_in_verilog() {
        digital_design_hardware::verify_verilog_with_iverilog::<BoardHealthProbe>().unwrap();
    }
}
