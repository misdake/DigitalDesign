use digital_design_circuit::CircuitWires;
use digital_design_hardware_gowin::{
    run_gowin_project_cli, GowinCliError, GowinModuleProject, Hardware, Module, TangNano20K,
    TangNano20KSdramInputs, TangNano20KSdramOutputs,
};

fn main() -> Result<(), GowinCliError> {
    run_gowin_project_cli(sdram_gowin_project(), "target/sdram_gowin")
}

#[derive(Hardware)]
#[hardware(namespace = "examples/sdram")]
struct SdramBoardSelfTest;

impl Module for SdramBoardSelfTest {
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
        panic!("SdramBoardSelfTest is a Verilog-only hardware test harness")
    }

    fn verilog_source() -> Option<String> {
        Some(include_str!("self_test.v").to_string())
    }

    fn verilog_testbench() -> Option<String> {
        Some(include_str!("signature_testbench.v").to_string())
    }
}

fn sdram_gowin_project() -> GowinModuleProject<TangNano20K, SdramBoardSelfTest> {
    TangNano20K::sdram_debug_uart_project::<SdramBoardSelfTest>("sdram_self_test")
}

#[cfg(test)]
mod tests {
    use super::*;
    use digital_design_hardware::ResourceKind;

    #[test]
    fn project_contains_the_controller_clock_and_fitted_memory() {
        let project = sdram_gowin_project().generate().unwrap();
        assert_eq!(project.resources.claimed[&ResourceKind::SdrSdramDevice], 1);
        assert_eq!(project.resources.claimed[&ResourceKind::Pll], 1);
        assert!(project
            .files
            .values()
            .any(|source| source.contains("SDRAM_Controller_HS_QN88 u_sdram_controller")));
        assert!(project
            .files
            .values()
            .any(|source| source.contains("TangNano20KSdramPll54M u_sdram_pll")));
        assert!(project.files[std::path::Path::new("build.tcl")]
            .contains("{ipcore} {SDRC_HS} {data} {sdrc_hs_top.vp}"));
    }
}
