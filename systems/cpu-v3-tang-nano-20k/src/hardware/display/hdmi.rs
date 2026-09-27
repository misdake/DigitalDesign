//! Multi-clock framebuffer fetch, line buffering, and HDMI TMDS output. The
//! scanout timing is a compile-time [`DisplayConfig`]: the active mode is
//! injected into the RTL and its testbench, so both always follow the single
//! `ACTIVE_DISPLAY_CONFIG` constant.

use crate::display::{DisplayConfig, ACTIVE_DISPLAY_CONFIG};
use crate::{DisplayLineBuffer, DisplayPairFifo};
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
    pub memory_read_data: Wires<64>,
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
        verilog_source_for(&ACTIVE_DISPLAY_CONFIG)
    }

    fn verilog_dependencies() -> Vec<VerilogDependency> {
        vec![
            VerilogDependency::new::<DisplayLineBuffer>("u_line_buffer"),
            VerilogDependency::new::<DisplayPairFifo>("u_pair_fifo"),
        ]
    }

    fn verilog_testbench() -> Option<String> {
        verilog_testbench_for(&ACTIVE_DISPLAY_CONFIG)
    }
}

fn verilog_source_for(config: &DisplayConfig) -> Option<String> {
    Some(
        include_str!("display_hdmi.v")
            .replace("__DISPLAY_CONFIG__", &config.verilog_localparams())
            .replace(
                "__LINE_BUFFER__",
                &DisplayLineBuffer::verilog_identity().module_name(),
            )
            .replace(
                "__PAIR_FIFO__",
                &DisplayPairFifo::verilog_identity().module_name(),
            ),
    )
}

fn verilog_testbench_for(config: &DisplayConfig) -> Option<String> {
    Some(
        include_str!("display_hdmi_tb.v")
            .replace("__DISPLAY_CONFIG__", &config.verilog_localparams())
            .replace(
                "__PIXEL_HALF_PERIOD__",
                &(500_000_000.0 / config.pixel_clock_hz as f64).to_string(),
            )
            .to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::display::DISPLAY_MODES;
    use digital_design_hardware::{ResourceKind, VerilogProject};

    #[test]
    fn display_claims_two_line_buffer_blocks() {
        let project = VerilogProject::generate::<FramebufferHdmi>().unwrap();
        let amount = |kind| {
            project
                .resource_claims
                .iter()
                .flat_map(|claim| &claim.resources)
                .filter(|resource| resource.kind == kind)
                .map(|resource| resource.amount)
                .sum::<u64>()
        };
        assert_eq!(amount(ResourceKind::Bsram18K), 2);
        assert_eq!(amount(ResourceKind::SsramBit), 12 * 64);
    }

    #[test]
    fn generated_verilog_embeds_the_selected_mode_timing() {
        let config = ACTIVE_DISPLAY_CONFIG;
        let source = verilog_source_for(&config).unwrap();
        assert!(source.contains(&format!("localparam [10:0] H_TOTAL={};", config.h_total)));
        assert!(source.contains(&format!("localparam [9:0] V_TOTAL={};", config.v_total)));
        assert!(source.contains(&format!(
            "localparam [8:0] SIDE_BORDER={};",
            config.side_border
        )));
        assert!(source.contains(&format!("localparam [1:0] SCALE={};", config.scale)));
        assert!(source.contains("localparam [9:0] LINE_SLOT_WORDS=FB_WIDTH/2;"));
        let testbench = verilog_testbench_for(&config).unwrap();
        assert!(testbench.contains(&format!("localparam [1:0] SCALE={};", config.scale)));
    }

    #[test]
    fn both_display_modes_inject_their_localparam_blocks() {
        for config in DISPLAY_MODES {
            assert!(config.verilog_localparams().contains("H_TOTAL"));
            assert!(config.verilog_localparams().contains("V_TOTAL"));
            let source = verilog_source_for(&config).unwrap();
            assert!(!source.contains("__DISPLAY_CONFIG__"));
            assert!(source.contains(&format!("H_ACTIVE_END={}", config.h_active_end)));
            assert!(!source.contains("__PAIR_"));
            assert!(source.contains("fifo_count<3"));
            let testbench = verilog_testbench_for(&config).unwrap();
            assert!(!testbench.contains("__DISPLAY_CONFIG__"));
        }
    }

    #[test]
    #[ignore = "explicit external simulation of active HDMI timing and burst fetch"]
    fn framebuffer_hdmi_runs_in_iverilog() {
        digital_design_hardware::verify_verilog_with_iverilog::<FramebufferHdmi>().unwrap();
    }

    #[derive(Hardware)]
    #[hardware(namespace = "tests/display_720p")]
    struct FramebufferHdmi720p;

    impl Module for FramebufferHdmi720p {
        type Input = FramebufferHdmiInput;
        type Output = FramebufferHdmiOutput;
        type EmuState = ();
        const USES_MAIN_CLOCK: bool = true;
        const EMU_AVAILABLE: bool = false;
        fn execute_emu(_: &mut (), _: &mut CircuitWires, _: &Self::Input, _: &Self::Output) {
            panic!("Verilog-only test");
        }
        fn verilog_source() -> Option<String> {
            verilog_source_for(&crate::display::VGA_720P_3X)
                .map(|s| s.replace("FramebufferHdmi", "FramebufferHdmi720p"))
        }
        fn verilog_testbench() -> Option<String> {
            verilog_testbench_for(&crate::display::VGA_720P_3X)
                .map(|s| s.replace("FramebufferHdmi", "FramebufferHdmi720p"))
        }
        fn verilog_dependencies() -> Vec<VerilogDependency> {
            FramebufferHdmi::verilog_dependencies()
        }
    }

    #[test]
    #[ignore = "explicit full-frame scanout in the alternative 3x video mode"]
    fn framebuffer_hdmi_720p_runs_in_iverilog() {
        digital_design_hardware::verify_verilog_with_iverilog::<FramebufferHdmi720p>().unwrap();
    }

    #[derive(Hardware)]
    #[hardware(namespace = "tests/display_fault")]
    struct FramebufferHdmiFault;
    impl Module for FramebufferHdmiFault {
        type Input = FramebufferHdmiInput;
        type Output = FramebufferHdmiOutput;
        type EmuState = ();
        const USES_MAIN_CLOCK: bool = true;
        const EMU_AVAILABLE: bool = false;
        fn execute_emu(_: &mut (), _: &mut CircuitWires, _: &Self::Input, _: &Self::Output) {
            panic!("Verilog-only test");
        }
        fn verilog_source() -> Option<String> {
            verilog_source_for(&crate::display::VGA_800X480_2X)
                .map(|s| s.replace("FramebufferHdmi", "FramebufferHdmiFault"))
        }
        fn verilog_testbench() -> Option<String> {
            Some(include_str!("display_fault_tb.v").to_string())
        }
        fn verilog_dependencies() -> Vec<VerilogDependency> {
            FramebufferHdmi::verilog_dependencies()
        }
    }
    #[test]
    #[ignore = "external failed-fill, malformed-LAST and scanout underflow scenarios"]
    fn framebuffer_faults_run_in_iverilog() {
        digital_design_hardware::verify_verilog_with_iverilog::<FramebufferHdmiFault>().unwrap();
    }
}
