//! One inferred 18-Kbit, dual-clock line buffer for three 320-pixel lines.

use digital_design_circuit::{CircuitWires, Wire, Wires};
use digital_design_hardware::{BsramBlocks, Hardware, Module, ModuleIo, TargetResourceRequest};

#[derive(Clone, ModuleIo)]
pub struct DisplayLineBufferInput {
    pub write_clock: Wire,
    pub write_enable: Wire,
    pub write_address: Wires<9>,
    pub write_data: Wires<32>,
    pub read_clock: Wire,
    pub read_address: Wires<9>,
}

#[derive(Clone, ModuleIo)]
pub struct DisplayLineBufferOutput {
    pub read_data: Wires<32>,
}

#[derive(Hardware)]
#[hardware(namespace = "systems/cpu_v3_tang_nano_20k/display", target_leaf)]
pub struct DisplayLineBuffer;

impl Module for DisplayLineBuffer {
    type Input = DisplayLineBufferInput;
    type Output = DisplayLineBufferOutput;
    type EmuState = ();

    const EMU_AVAILABLE: bool = false;

    fn target_resources() -> Vec<TargetResourceRequest> {
        vec![TargetResourceRequest::new(BsramBlocks::new(1))]
    }

    fn execute_emu(
        _state: &mut Self::EmuState,
        _circuit: &mut CircuitWires,
        _input: &Self::Input,
        _output: &Self::Output,
    ) {
        panic!("dual-clock line buffer is Verilog-only")
    }

    fn verilog_source() -> Option<String> {
        Some(include_str!("display_line_buffer.v").to_string())
    }

    fn verilog_testbench() -> Option<String> {
        Some(include_str!("display_line_buffer_tb.v").to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "explicit external simulation of the dual-clock inferred RAM"]
    fn dual_clock_ram_runs_in_iverilog() {
        digital_design_hardware::verify_verilog_with_iverilog::<DisplayLineBuffer>().unwrap();
    }
}
