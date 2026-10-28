//! Streaming endpoint with two ordered descriptors and independent payload retirement.
use digital_design_circuit::{CircuitWires, Wire, Wires};
use digital_design_hardware::{Hardware, HardwareIdentity, Module, ModuleIo};

#[derive(Clone, ModuleIo)]
pub struct StreamingSdramPortInput {
    pub reset: Wire,
    pub cpu_request_valid: Wire,
    pub cpu_write: Wire,
    pub cpu_line: Wire,
    pub cpu_address: Wires<22>,
    pub cpu_line_count_minus_1: Wires<2>,
    pub cpu_chain_class: Wires<2>,
    pub cpu_write_data: Wires<64>,
    pub cpu_response_ready: Wire,
    pub controller_read_data: Wires<64>,
    pub controller_read_valid: Wire,
    pub controller_init_done: Wire,
    pub controller_request_ready: Wire,
    pub controller_stream_active: Wire,
    pub controller_write_done: Wire,
    pub controller_write_data_ready: Wire,
}
#[derive(Clone, ModuleIo)]
pub struct StreamingSdramPortOutput {
    pub cpu_request_ready: Wire,
    pub cpu_lookahead_window: Wire,
    pub cpu_write_data_ready: Wire,
    pub cpu_response_valid: Wire,
    pub cpu_read_data: Wires<64>,
    pub cpu_response_last: Wire,
    pub cpu_error: Wire,
    pub controller_request_valid: Wire,
    pub controller_next_valid: Wire,
    pub controller_next_address: Wires<21>,
    pub controller_request_continuation: Wire,
    pub controller_write: Wire,
    pub controller_address: Wires<21>,
    pub controller_write_mask: Wires<4>,
    pub controller_write_data: Wires<64>,
    pub controller_write_data_valid: Wire,
    pub controller_words: Wires<6>,
}
#[derive(Hardware)]
#[hardware(namespace = "hardware/gowin/sdram")]
pub struct StreamingSdramPort;
impl Module for StreamingSdramPort {
    type Input = StreamingSdramPortInput;
    type Output = StreamingSdramPortOutput;
    type EmuState = ();
    const USES_MAIN_CLOCK: bool = true;
    const EMU_AVAILABLE: bool = false;
    fn execute_emu(_: &mut (), _: &mut CircuitWires, _: &Self::Input, _: &Self::Output) {
        panic!("Streaming SDRAM module is RTL-only; use sim::endpoint for functional reference")
    }
    fn verilog_source() -> Option<String> {
        Some(super::RtlSources::STREAM_SHARED_PORT.replace(
            "module SharedSdramPort",
            &format!("module {}", Self::verilog_identity().module_name()),
        ))
    }
    fn verilog_testbench() -> Option<String> {
        Some(
            include_str!("rtl/streaming_port_tb.v")
                .replace("__DUT__", &Self::verilog_identity().module_name()),
        )
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "bounded streaming endpoint read continuation and held scalar completion"]
    fn stream_endpoint_in_verilog() {
        digital_design_hardware::verify_verilog_with_iverilog::<super::StreamingSdramPort>()
            .unwrap();
    }
}
