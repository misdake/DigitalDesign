//! Multi-clock framebuffer fetch, line buffering, and HDMI TMDS output. The
//! fixed 800x480 2x timing is a shared [`DisplayConfig`]: the timing is
//! injected into the RTL and its testbench, so both always follow the single
//! `ACTIVE_DISPLAY_CONFIG` constant.

use super::line_buffer::TMDS_PRECODE_BITS;
use crate::display::{DisplayConfig, ACTIVE_DISPLAY_CONFIG};
use crate::DisplayLineBuffer;
use digital_design_circuit::{CircuitWires, Wire, Wires};
use digital_design_hardware::{Hardware, HardwareIdentity, Module, ModuleIo, VerilogDependency};

#[derive(Clone, ModuleIo)]
pub struct FramebufferHdmiInput {
    pub reset: Wire,
    pub pixel_clock: Wire,
    pub serial_clock: Wire,
    pub video_locked: Wire,
    pub boot_enable: Wire,
    pub boot_failed: Wire,
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
    pub started_ack: Wire,
    pub startup_error: Wire,
    pub frame_event: Wire,
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
        verilog_source_for(&ACTIVE_DISPLAY_CONFIG).map(|source| {
            if matches!(
                option_env!("CPU_V3_GPU_PROFILE"),
                Some("texture16x8-column16")
            ) {
                column16_source(source)
            } else {
                source
            }
        })
    }

    fn verilog_dependencies() -> Vec<VerilogDependency> {
        vec![VerilogDependency::new::<DisplayLineBuffer>("u_line_buffer")]
    }

    fn verilog_testbench() -> Option<String> {
        verilog_testbench_for(&ACTIVE_DISPLAY_CONFIG).map(|source| {
            if matches!(
                option_env!("CPU_V3_GPU_PROFILE"),
                Some("texture16x8-column16")
            ) {
                column16_testbench(source)
            } else {
                source
            }
        })
    }
}

// Only macro ordering changes.32-byte row requests and line-buffer ownership
// remain unchanged; one tile column now advances4096 halfwords, not256.
fn column16_source(mut source: String) -> String {
    for (old, new) in [
        ("TILE_ROW_STRIDE=FB_WIDTH*16", "TILE_ROW_STRIDE=13'd256"),
        (
            "{9'b0,burst_index,local_y[3:1],fill_second}",
            "{5'b0,burst_index,4'b0,local_y[3:1],fill_second}",
        ),
        ("32'h003e8900", "32'h003e7000"),
    ] {
        assert_eq!(
            source.matches(old).count(),
            1,
            "column Display anchor {old}"
        );
        source = source.replacen(old, new, 1);
    }
    source
}
fn column16_testbench(source: String) -> String {
    source
        .replace(
            "(source_row/16)*6400+(source_row%16)*16+source_tile_x*256",
            "(source_row/16)*256+(source_row%16)*16+source_tile_x*4096",
        )
        .replace("23'd96000", "23'd102144")
        .replace("3e8900", "3e7000")
        .replace("16'h8900", "16'h7000")
        .replace("16'h8901", "16'h7001")
        .replace("16'h8910", "16'h7010")
}

fn verilog_source_for(config: &DisplayConfig) -> Option<String> {
    Some(
        include_str!("display_hdmi.v")
            .replace("__TMDS_PRECODE_BITS__", &TMDS_PRECODE_BITS.to_string())
            .replace("__DISPLAY_CONFIG__", &config.verilog_localparams())
            .replace(
                "__LINE_BUFFER__",
                &DisplayLineBuffer::verilog_identity().module_name(),
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
        assert_eq!(amount(ResourceKind::SsramBit), 0);
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
    fn fixed_display_injects_its_localparam_block() {
        {
            let config = ACTIVE_DISPLAY_CONFIG;
            assert!(config.verilog_localparams().contains("H_TOTAL"));
            assert!(config.verilog_localparams().contains("V_TOTAL"));
            let source = verilog_source_for(&config).unwrap();
            assert!(!source.contains("__DISPLAY_CONFIG__"));
            assert!(source.contains(&format!("H_ACTIVE_END={}", config.h_active_end)));
            assert!(!source.contains("__PAIR_"));
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
