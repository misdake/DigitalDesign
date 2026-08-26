//! Shared CPU word/display burst adapter for the fitted Controller HS port.

use crate::DisplayGrant;
use digital_design_circuit::{CircuitWires, Wire, Wires};
use digital_design_hardware::{Hardware, HardwareIdentity, Module, ModuleIo, VerilogDependency};

#[derive(Clone, ModuleIo)]
pub struct DisplaySdramPortInput {
    pub reset: Wire,
    pub cpu_request_valid: Wire,
    pub cpu_write: Wire,
    pub cpu_address: Wires<22>,
    pub cpu_write_data: Wires<16>,
    pub cpu_response_ready: Wire,
    pub display_request_valid: Wire,
    pub display_urgent: Wire,
    pub display_address: Wires<22>,
    pub controller_read_data: Wires<32>,
    pub controller_read_valid: Wire,
    pub controller_init_done: Wire,
    pub controller_command_ack: Wire,
}

#[derive(Clone, ModuleIo)]
pub struct DisplaySdramPortOutput {
    pub cpu_request_ready: Wire,
    pub cpu_response_valid: Wire,
    pub cpu_read_data: Wires<16>,
    pub cpu_error: Wire,
    pub display_request_ready: Wire,
    pub display_data_valid: Wire,
    pub display_read_data: Wires<32>,
    pub display_last: Wire,
    pub display_error: Wire,
    pub controller_command_valid: Wire,
    pub controller_command: Wires<3>,
    pub controller_precharge: Wire,
    pub controller_address: Wires<21>,
    pub controller_write_mask: Wires<4>,
    pub controller_write_data: Wires<32>,
    pub controller_burst_length: Wires<8>,
}

#[derive(Hardware)]
#[hardware(namespace = "systems/cpu_v3_tang_nano_20k/display")]
pub struct DisplaySdramPort;

impl Module for DisplaySdramPort {
    type Input = DisplaySdramPortInput;
    type Output = DisplaySdramPortOutput;
    type EmuState = ();

    const USES_MAIN_CLOCK: bool = true;
    const EMU_AVAILABLE: bool = false;

    fn execute_emu(
        _state: &mut Self::EmuState,
        _circuit: &mut CircuitWires,
        _input: &Self::Input,
        _output: &Self::Output,
    ) {
        panic!("DisplaySdramPort uses the host display scheduler for emulation")
    }

    fn verilog_source() -> Option<String> {
        Some(include_str!("display_sdram.v").replace(
            "__DISPLAY_GRANT__",
            &DisplayGrant::verilog_identity().module_name(),
        ))
    }

    fn verilog_dependencies() -> Vec<VerilogDependency> {
        vec![VerilogDependency::new::<DisplayGrant>("u_grant")]
    }

    fn verilog_testbench() -> Option<String> {
        Some(include_str!("display_sdram_tb.v").to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use digital_design_hardware::VerilogProject;

    #[test]
    fn export_contains_gate_grant_dependency() {
        let project = VerilogProject::generate::<DisplaySdramPort>().unwrap();
        assert_eq!(project.files.len(), 2);
        assert!(project.resource_claims.is_empty());
    }

    #[test]
    #[ignore = "explicit external simulation of shared SDRAM timing"]
    fn shared_word_and_burst_port_runs_in_iverilog() {
        digital_design_hardware::verify_verilog_with_iverilog::<DisplaySdramPort>().unwrap();
    }
}
