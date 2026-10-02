//! On-board traffic generator and latency counters. No CPU/GPU IP dependency.
use super::{
    CpuV3MemoryArbiter, CpuV3MemoryArbiterInput, CpuV3MemoryArbiterOutput, SharedSdramPort,
};
use crate::{
    GowinModuleProject, TangNano20K, TangNano20KSdramWideInputs, TangNano20KSdramWideOutputs,
};
use digital_design_circuit::CircuitWires;
use digital_design_hardware::{Hardware, HardwareIdentity, Module, ModuleIo, VerilogDependency};
#[derive(Hardware)]
#[hardware(namespace = "vendor/gowin/sdram")]
pub struct SdramTrafficProbe;
impl Module for SdramTrafficProbe {
    type Input = TangNano20KSdramWideInputs;
    type Output = TangNano20KSdramWideOutputs;
    type EmuState = ();
    const USES_MAIN_CLOCK: bool = true;
    const EMU_AVAILABLE: bool = false;
    fn execute_emu(_: &mut (), _: &mut CircuitWires, _: &Self::Input, _: &Self::Output) {
        panic!("run the connected cycle/pin fixture for the board traffic probe");
    }
    fn verilog_source() -> Option<String> {
        Some(Self::source(1 << 20, 469))
    }
    fn verilog_dependencies() -> Vec<VerilogDependency> {
        vec![
            VerilogDependency::new::<CpuV3MemoryArbiter>("arbiter"),
            VerilogDependency::new::<SharedSdramPort>("adapter"),
        ]
    }
    fn verilog_testbench() -> Option<String> {
        Some(format!(
            "{}\n{}\n{}\n{}",
            include_str!("rtl/traffic_probe_tb.v"),
            super::RtlSources::GEARBOX,
            super::RtlSources::CONTROLLER,
            super::RtlSources::PIN_MODEL
        ))
    }
}
impl SdramTrafficProbe {
    /// Simulation can shorten the sampling window/UART divisor; board defaults
    /// are 2^20 logic clocks and 115200 baud. Clock frequency stays target-owned.
    pub fn source(window: u32, uart_div: u32) -> String {
        assert!((1..=1 << 24).contains(&window));
        assert!((1..=65535).contains(&uart_div));
        include_str!("rtl/traffic_probe.v")
            .replace(
                "__ARBITER__",
                &CpuV3MemoryArbiter::verilog_identity().module_name(),
            )
            .replace(
                "__SHARED_PORT__",
                &SharedSdramPort::verilog_identity().module_name(),
            )
            .replace("__WINDOW__", &window.to_string())
            .replace("__UART_DIV__", &uart_div.to_string())
            .replace("__CLIENT_CONNECTIONS__", &Self::connections())
    }
    fn connections() -> String {
        let i = super::emu::idle_inputs();
        let o = super::arbiter::compute_output(&Default::default(), &i);
        let ins = CpuV3MemoryArbiterInput::verilog_values(&i);
        let outs = CpuV3MemoryArbiterOutput::verilog_values(&o);
        let prefixes = [
            "display",
            "instruction",
            "data",
            "dma",
            "gpu_ro",
            "gpu_fb_r",
            "gpu_fb_w",
        ];
        let mut links = vec![".clk(clk)".to_string()];
        for v in ins.iter().chain(&outs) {
            let mut connection = v.name.to_string();
            for (n, p) in prefixes.iter().enumerate() {
                if let Some(field) = v.name.strip_prefix(&format!("{p}_")) {
                    connection = match field {
                        "request_valid" => format!("pending[{n}] && !active[{n}]"),
                        "address" => format!("addresses[{n}]"),
                        "write" => format!("writes[{n}]"),
                        "line" => "1'b1".into(),
                        "line_count_minus_1" => {
                            if n >= 4 {
                                "2'd3".into()
                            } else {
                                "2'd0".into()
                            }
                        }
                        "write_data" => {
                            if n == 3 {
                                "dma_payload".into()
                            } else {
                                format!("payload[{n}]")
                            }
                        }
                        "response_ready" => "1'b1".into(),
                        "request_ready" => format!("request_ready[{n}]"),
                        "write_data_ready" => format!("feed_ready[{n}]"),
                        "response_valid" => format!("response_valid[{n}]"),
                        "read_data" => {
                            if n == 3 {
                                "dma_result".into()
                            } else {
                                format!("response_data[{n}]")
                            }
                        }
                        "response_last" => format!("response_last[{n}]"),
                        "error" => format!("response_error[{n}]"),
                        _ => panic!("unexpected client field {field}"),
                    };
                }
            }
            links.push(format!(".{}({connection})", v.name));
        }
        links.join(",\n")
    }
    pub fn project() -> GowinModuleProject<TangNano20K, Self> {
        TangNano20K::sdram_wide_debug_uart_project::<Self>("sdram_traffic_probe")
    }
}
