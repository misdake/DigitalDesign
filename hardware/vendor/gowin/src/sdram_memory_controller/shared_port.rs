//! Single-upstream CPU word/line adapter for the native SDRAM request port.
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
    pub controller_request_ready: Wire,
    pub controller_done: Wire,
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
    pub controller_request_valid: Wire,
    pub controller_write: Wire,
    pub controller_address: Wires<21>,
    pub controller_write_mask: Wires<4>,
    pub controller_write_data: Wires<64>,
    pub controller_write_data_valid: Wire,
    pub controller_words: Wires<6>,
}

#[derive(Hardware)]
#[hardware(namespace = "systems/cpu_v3_tang_nano_20k/display")]
pub struct SharedSdramPort;

impl Module for SharedSdramPort {
    type Input = SharedSdramPortInput;
    type Output = SharedSdramPortOutput;
    type EmuState = super::emu::adapter::State;

    const USES_MAIN_CLOCK: bool = true;
    const EMU_AVAILABLE: bool = true;

    fn create_emu(_input: &Self::Input, _output: &Self::Output) -> Self::EmuState {
        Self::EmuState::default()
    }

    fn execute_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        input: &Self::Input,
        output: &Self::Output,
    ) {
        output.drive(circuit, &state.output(&input.sample(circuit)));
    }

    fn clock_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        input: &Self::Input,
        _output: &Self::Output,
    ) {
        state.clock(&input.sample(circuit));
    }

    fn verilog_source() -> Option<String> {
        Some(include_str!("rtl/display_sdram.v").to_string())
    }

    fn verilog_testbench() -> Option<String> {
        Some(include_str!("rtl/display_sdram_tb.v").to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use digital_design_hardware::VerilogProject;
    use std::path::Path;
    use std::process::Command;

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

    #[test]
    #[ignore = "explicit CL2 pin-model validation of the related-clock native bridge"]
    fn native_bridge_bank_striping_runs_in_iverilog() {
        let vendor = Path::new(env!("CARGO_MANIFEST_DIR"));
        let display = vendor.join("src/sdram_memory_controller/rtl");
        let controller = display.clone();
        let executable =
            std::env::temp_dir().join(format!("cpu-v3-native-sdram-{}.vvp", std::process::id()));
        let iverilog = std::env::var_os("IVERILOG_EXE").unwrap_or_else(|| "iverilog".into());
        let vvp = std::env::var_os("VVP_EXE").unwrap_or_else(|| "vvp".into());
        let compile = Command::new(iverilog)
            .args(["-g2012", "-s", "tb", "-o"])
            .arg(&executable)
            .arg(controller.join("sdram_controller.v"))
            .arg(controller.join("native_bridge_108m_54m.v"))
            .arg(display.join("sdram_pin_model.v"))
            .arg(display.join("sdram_native_bridge_tb.v"))
            .output()
            .unwrap();
        assert!(
            compile.status.success(),
            "Icarus compile: {}",
            String::from_utf8_lossy(&compile.stderr)
        );
        let run = Command::new(vvp).arg(&executable).output().unwrap();
        let _ = std::fs::remove_file(&executable);
        let stdout = String::from_utf8_lossy(&run.stdout);
        assert!(
            run.status.success() && stdout.contains("PASS native 64/32 bridge"),
            "Icarus run: {stdout} {}",
            String::from_utf8_lossy(&run.stderr)
        );
    }
}
