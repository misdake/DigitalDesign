use cpu_v3_tang_nano_20k::SystemControlDevice;
use digital_design_circuit::CircuitWires;
use digital_design_hardware::{Hardware, Module, VerilogDependency};
use digital_design_hardware_gowin::{
    run_gowin_project_cli, GowinCliError, TangNano20K, TangNano20KDebugOutputs, TangNano20KInputs,
};

fn main() -> Result<(), GowinCliError> {
    run_gowin_project_cli(gowin_project(), "target/sysctl_uart_gowin")
}

/// Board characterization for the system-control device with no CPU involved:
/// an FSM drives its register interface directly and emits the DDHT test ID
/// `0x08` frame on the debug UART. 27 MHz project, so 234 clocks per bit.
type SystemControl = SystemControlDevice<234>;

#[derive(Hardware)]
#[hardware(namespace = "examples/sysctl_uart")]
struct SysctlUartProbe;

impl Module for SysctlUartProbe {
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
        panic!("SysctlUartProbe is a Verilog-only hardware characterization harness")
    }

    fn verilog_source() -> Option<String> {
        Some(include_str!("self_test.v").to_string())
    }

    fn verilog_dependencies() -> Vec<VerilogDependency> {
        vec![VerilogDependency::new::<SystemControl>("u_sysctl")]
    }

    fn verilog_testbench() -> Option<String> {
        Some(include_str!("probe_testbench.v").to_string())
    }
}

fn gowin_project() -> digital_design_hardware_gowin::GowinModuleProject<TangNano20K, SysctlUartProbe>
{
    TangNano20K::debug_uart_project::<SysctlUartProbe>("sysctl_uart_probe")
}

#[cfg(test)]
mod tests {
    use super::*;
    use digital_design_hardware::VerilogProject;

    #[test]
    fn project_contains_only_the_system_control_device() {
        let verilog = VerilogProject::generate::<SysctlUartProbe>().unwrap();
        assert_eq!(verilog.resource_claims.len(), 0);
        let project = gowin_project().generate().unwrap();
        assert!(project.files.contains_key(std::path::Path::new(
            "src/generated/components/system_control/system_control_device/clocks_per_bit234.v"
        )));
    }

    #[test]
    #[ignore = "explicit external simulator validation"]
    fn probe_frame_decodes_in_verilog() {
        digital_design_hardware::verify_verilog_with_iverilog::<SysctlUartProbe>().unwrap();
    }
}
