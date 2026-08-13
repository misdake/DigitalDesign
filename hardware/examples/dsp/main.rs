use digital_design_code::CircuitWires;
use digital_design_hardware::{
    run_gowin_project_cli, DspMacS18, DspMulAddS18, DspMulS18, GowinCliError, GowinModuleProject,
    Hardware, Module, ModuleTest, TangNano20K, TangNano20KDebugOutputs,
    TangNano20KDebugOutputsValue, TangNano20KInputs, TangNano20KInputsValue, TestStep,
    VerilogDependency,
};

fn main() -> Result<(), GowinCliError> {
    run_gowin_project_cli(dsp_gowin_project(), "target/dsp_gowin")
}

#[derive(Hardware)]
#[hardware(namespace = "examples/dsp")]
struct DspBoardSelfTest;

impl Module for DspBoardSelfTest {
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
        panic!("DspBoardSelfTest is a Verilog-only hardware test harness")
    }

    fn verilog_source() -> Option<String> {
        Some(include_str!("self_test.v").to_string())
    }

    fn verilog_dependencies() -> Vec<VerilogDependency> {
        vec![
            VerilogDependency::new::<DspMulS18>("u_mul"),
            VerilogDependency::new::<DspMulAddS18>("u_mul_add"),
            VerilogDependency::new::<DspMacS18>("u_mac"),
        ]
    }

    fn verilog_testbench() -> Option<String> {
        Some(board_test().verilog_testbench())
    }
}

fn board_test() -> ModuleTest<DspBoardSelfTest> {
    ModuleTest::new([TestStep::new(
        TangNano20KInputsValue { buttons: 0 },
        TangNano20KDebugOutputsValue {
            leds: 1,
            uart_tx: true,
        },
    )
    .after_cycles(12)])
}

fn dsp_gowin_project() -> GowinModuleProject<TangNano20K, DspBoardSelfTest> {
    TangNano20K::debug_uart_project::<DspBoardSelfTest>("dsp_self_test")
}

#[cfg(test)]
mod tests {
    use super::*;
    use digital_design_hardware::{HardwareIdentity, ResourceKind, VerilogProject};

    #[test]
    fn project_contains_the_measured_dsp_shapes() {
        let verilog = VerilogProject::generate::<DspBoardSelfTest>().unwrap();
        for (module, instance) in [
            (DspMulS18::verilog_identity().module_name(), "u_mul"),
            (DspMulAddS18::verilog_identity().module_name(), "u_mul_add"),
            (DspMacS18::verilog_identity().module_name(), "u_mac"),
        ] {
            assert!(verilog
                .files
                .values()
                .any(|source| source.contains(&format!("module {module}("))));
            assert!(verilog
                .files
                .values()
                .any(|source| source.contains(&format!("{module} {instance}"))));
        }
        assert_eq!(verilog.resource_claims.len(), 3);

        let project = dsp_gowin_project().generate().unwrap();
        assert_eq!(project.resources.claimed[&ResourceKind::Multiplier18x18], 5);
        assert_eq!(project.resources.claimed[&ResourceKind::DebugUartTx], 1);
    }

    #[test]
    #[ignore = "explicit external simulator validation"]
    fn verify_board_harness_with_iverilog() {
        digital_design_hardware::verify_verilog_with_iverilog::<DspBoardSelfTest>().unwrap();
    }
}
