use digital_design_circuit::CircuitWires;
use digital_design_hardware_gowin::{
    run_gowin_project_cli, GowinCliError, Hardware, HardwareIdentity, Module, TangNano20K,
    TangNano20KSdramInputs, TangNano20KSdramOutputs, TangNano20KSdramWordPort, VerilogDependency,
};

fn main() -> Result<(), GowinCliError> {
    run_gowin_project_cli(
        TangNano20K::sdram_debug_uart_project::<SdramWordPortSelfTest>("sdram_word_port_self_test"),
        "target/sdram_word_port_gowin",
    )
}

#[derive(Hardware)]
#[hardware(namespace = "examples/sdram_word_port")]
struct SdramWordPortSelfTest;

impl Module for SdramWordPortSelfTest {
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
        panic!("SdramWordPortSelfTest is a Verilog-only board test")
    }

    fn verilog_source() -> Option<String> {
        Some(include_str!("self_test.v").replace(
            "__SDRAM_WORD_PORT__",
            &TangNano20KSdramWordPort::verilog_identity().module_name(),
        ))
    }

    fn verilog_dependencies() -> Vec<VerilogDependency> {
        vec![VerilogDependency::new::<TangNano20KSdramWordPort>(
            "u_memory",
        )]
    }

    fn verilog_testbench() -> Option<String> {
        Some(include_str!("signature_testbench.v").to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use digital_design_hardware_gowin::{ResourceKind, VerilogProject};

    #[test]
    fn board_test_uses_the_target_adapter_without_extra_memory_claims() {
        let verilog = VerilogProject::generate::<SdramWordPortSelfTest>().unwrap();
        assert!(verilog.resource_claims.is_empty());
        let project = TangNano20K::sdram_debug_uart_project::<SdramWordPortSelfTest>("test")
            .generate()
            .unwrap();
        assert_eq!(project.resources.claimed[&ResourceKind::SdrSdramDevice], 1);
        assert_eq!(project.resources.claimed[&ResourceKind::Pll], 1);
    }

    #[test]
    #[ignore = "explicit external simulator validation"]
    fn verify_board_harness_with_iverilog() {
        digital_design_hardware::verify_verilog_with_iverilog::<SdramWordPortSelfTest>().unwrap();
    }
}
