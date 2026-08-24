use digital_design_circuit::CircuitWires;
use digital_design_hardware_gowin::{
    run_gowin_project_cli, GowinCliError, Hardware, Module, TangNano20K, TangNano20KHdmiInputs,
    TangNano20KHdmiOutputs,
};

fn main() -> Result<(), GowinCliError> {
    run_gowin_project_cli(
        TangNano20K::hdmi_project::<HdmiColorBars>("hdmi_color_bars"),
        "target/hdmi_color_bars_gowin",
    )
}

/// Stand-alone 1280x720p60 HDMI physical-link bring-up pattern.
#[derive(Hardware)]
#[hardware(namespace = "examples/hdmi_color_bars")]
struct HdmiColorBars;

impl Module for HdmiColorBars {
    type Input = TangNano20KHdmiInputs;
    type Output = TangNano20KHdmiOutputs;
    type EmuState = ();

    const USES_MAIN_CLOCK: bool = true;
    const EMU_AVAILABLE: bool = false;

    fn execute_emu(
        _state: &mut Self::EmuState,
        _circuit: &mut CircuitWires,
        _input: &Self::Input,
        _output: &Self::Output,
    ) {
        panic!("HdmiColorBars is a Verilog-only board probe")
    }

    fn verilog_source() -> Option<String> {
        Some(include_str!("self_test.v").to_string())
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
    fn project_claims_the_fitted_hdmi_port_and_one_pll() {
        let verilog = VerilogProject::generate::<HdmiColorBars>().unwrap();
        assert!(verilog.resource_claims.is_empty());
        let project = TangNano20K::hdmi_project::<HdmiColorBars>("hdmi_color_bars")
            .generate()
            .unwrap();
        assert_eq!(project.resources.claimed[&ResourceKind::HdmiOutput], 1);
        assert_eq!(project.resources.claimed[&ResourceKind::Pll], 1);
    }

    #[test]
    #[ignore = "explicit external simulator validation of 720p timing and TMDS control periods"]
    fn timing_and_tmds_control_codes_decode_in_iverilog() {
        digital_design_hardware::verify_verilog_with_iverilog::<HdmiColorBars>().unwrap();
    }
}
