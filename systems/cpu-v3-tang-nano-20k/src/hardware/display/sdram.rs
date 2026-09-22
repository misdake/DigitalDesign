//! Single-upstream CPU word/line adapter for the fitted Controller HS port.
//!
//! Display, I-cache, D-cache, DMA and the three GPU masters all share the
//! arbiter's one request stream, so this adapter no longer arbitrates between
//! a CPU and a display client.

use digital_design_circuit::{CircuitWires, Wire, Wires};
use digital_design_hardware::{Hardware, Module, ModuleIo};

#[derive(Clone, ModuleIo)]
pub struct SharedSdramPortInput {
    pub reset: Wire,
    pub cpu_request_valid: Wire,
    pub cpu_write: Wire,
    pub cpu_line: Wire,
    pub cpu_address: Wires<22>,
    pub cpu_line_count_minus_1: Wires<2>,
    pub cpu_write_data: Wires<64>,
    pub cpu_response_ready: Wire,
    pub controller_read_data: Wires<64>,
    pub controller_read_valid: Wire,
    pub controller_init_done: Wire,
    pub controller_command_ack: Wire,
    pub controller_write_data_ready: Wire,
}

#[derive(Clone, ModuleIo)]
pub struct SharedSdramPortOutput {
    pub cpu_request_ready: Wire,
    pub cpu_write_data_ready: Wire,
    pub cpu_response_valid: Wire,
    pub cpu_read_data: Wires<64>,
    pub cpu_response_last: Wire,
    pub cpu_error: Wire,
    pub controller_command_valid: Wire,
    pub controller_command: Wires<3>,
    pub controller_precharge: Wire,
    pub controller_address: Wires<21>,
    pub controller_write_mask: Wires<4>,
    pub controller_write_data: Wires<64>,
    pub controller_write_data_valid: Wire,
    pub controller_burst_length: Wires<8>,
}

#[derive(Hardware)]
#[hardware(namespace = "systems/cpu_v3_tang_nano_20k/display")]
pub struct SharedSdramPort;

impl Module for SharedSdramPort {
    type Input = SharedSdramPortInput;
    type Output = SharedSdramPortOutput;
    type EmuState = ();

    const USES_MAIN_CLOCK: bool = true;
    const EMU_AVAILABLE: bool = false;

    fn execute_emu(
        _state: &mut Self::EmuState,
        _circuit: &mut CircuitWires,
        _input: &Self::Input,
        _output: &Self::Output,
    ) {
        panic!("SharedSdramPort uses the host memory model for emulation")
    }

    fn verilog_source() -> Option<String> {
        Some(include_str!("display_sdram.v").to_string())
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
    fn export_is_one_standalone_module() {
        let project = VerilogProject::generate::<SharedSdramPort>().unwrap();
        assert_eq!(project.files.len(), 1);
        assert!(!project
            .files
            .values()
            .any(|source| source.contains("DisplayGrant")));
        assert!(project.resource_claims.is_empty());
    }

    #[test]
    #[ignore = "explicit external simulation of shared SDRAM timing"]
    fn shared_word_and_burst_port_runs_in_iverilog() {
        digital_design_hardware::verify_verilog_with_iverilog::<SharedSdramPort>().unwrap();
    }
}
