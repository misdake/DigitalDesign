//! Multi-clock framebuffer fetch, line buffering, and 720p TMDS output.

use crate::{DisplayLineBuffer, Rgb565ToRgb888};
use digital_design_circuit::{CircuitWires, Wire, Wires};
use digital_design_hardware::{Hardware, HardwareIdentity, Module, ModuleIo, VerilogDependency};

#[derive(Clone, ModuleIo)]
pub struct FramebufferHdmiInput {
    pub reset: Wire,
    pub pixel_clock: Wire,
    pub serial_clock: Wire,
    pub video_locked: Wire,
    pub memory_request_ready: Wire,
    pub memory_data_valid: Wire,
    pub memory_read_data: Wires<32>,
    pub memory_last: Wire,
    pub memory_error: Wire,
    pub device_index: Wires<3>,
    pub device_channel: Wires<4>,
    pub device_read_enable: Wire,
    pub device_write_enable: Wire,
    pub device_write_data: Wires<16>,
}

#[derive(Clone, ModuleIo)]
pub struct FramebufferHdmiOutput {
    pub memory_request_valid: Wire,
    pub memory_urgent: Wire,
    pub memory_address: Wires<22>,
    pub underflow: Wire,
    pub device_read_data: Wires<16>,
    pub tmds_clk_p: Wire,
    pub tmds_clk_n: Wire,
    pub tmds_data_p: Wires<3>,
    pub tmds_data_n: Wires<3>,
}

#[derive(Hardware)]
#[hardware(namespace = "systems/cpu_v3_tang_nano_20k/display")]
pub struct FramebufferHdmi;

impl Module for FramebufferHdmi {
    type Input = FramebufferHdmiInput;
    type Output = FramebufferHdmiOutput;
    type EmuState = ();
    const USES_MAIN_CLOCK: bool = true;
    const EMU_AVAILABLE: bool = false;

    fn execute_emu(
        _state: &mut Self::EmuState,
        _circuit: &mut CircuitWires,
        _input: &Self::Input,
        _output: &Self::Output,
    ) {
        panic!("FramebufferHdmi uses the host display model for emulation")
    }

    fn verilog_source() -> Option<String> {
        Some(
            include_str!("display_hdmi.v")
                .replace(
                    "__LINE_BUFFER__",
                    &DisplayLineBuffer::verilog_identity().module_name(),
                )
                .replace(
                    "__RGB565__",
                    &Rgb565ToRgb888::verilog_identity().module_name(),
                ),
        )
    }

    fn verilog_dependencies() -> Vec<VerilogDependency> {
        vec![
            VerilogDependency::new::<DisplayLineBuffer>("u_line_buffer"),
            VerilogDependency::new::<Rgb565ToRgb888>("u_rgb"),
        ]
    }

    fn verilog_testbench() -> Option<String> {
        Some(include_str!("display_hdmi_tb.v").to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use digital_design_hardware::{ResourceKind, VerilogProject};

    #[test]
    fn display_claims_one_line_buffer_block() {
        let project = VerilogProject::generate::<FramebufferHdmi>().unwrap();
        assert_eq!(project.resource_claims.len(), 1);
        assert_eq!(
            project.resource_claims[0].resources[0].kind,
            ResourceKind::Bsram18K
        );
    }

    #[test]
    #[ignore = "explicit external simulation of 720p timing and burst fetch"]
    fn framebuffer_hdmi_runs_in_iverilog() {
        digital_design_hardware::verify_verilog_with_iverilog::<FramebufferHdmi>().unwrap();
    }
}
