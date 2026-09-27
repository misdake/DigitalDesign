//! Four completed RGB888 pairs in twelve RAM16 cells.
//!
//! The physical width, rather than the four-entry depth, determines the cell
//! count. This separate HDL leaf matches the selected whole-system fit; changing
//! its structure requires another complete fit, since unrelated module mapping
//! changed in the smaller local alternatives. See the display optimization record.

use digital_design_circuit::{CircuitWires, Wire, Wires};
use digital_design_hardware::{Hardware, Module, ModuleIo, SsramBits, TargetResourceRequest};

#[derive(Clone, ModuleIo)]
pub struct DisplayPairFifoInput {
    pub write_clock: Wire,
    pub write_enable: Wire,
    pub write_address: Wires<2>,
    pub write_data: Wires<48>,
    pub read_address: Wires<2>,
}

#[derive(Clone, ModuleIo)]
pub struct DisplayPairFifoOutput {
    pub read_data: Wires<48>,
}

#[derive(Hardware)]
#[hardware(namespace = "systems/cpu_v3_tang_nano_20k/display", target_leaf)]
pub struct DisplayPairFifo;

impl Module for DisplayPairFifo {
    type Input = DisplayPairFifoInput;
    type Output = DisplayPairFifoOutput;
    type EmuState = ();
    const EMU_AVAILABLE: bool = false;

    fn target_resources() -> Vec<TargetResourceRequest> {
        // Each 16x4 cell supplies four bits of the asynchronous 48-bit read.
        vec![TargetResourceRequest::new(SsramBits::new(12 * 64))]
    }

    fn execute_emu(
        _state: &mut Self::EmuState,
        _circuit: &mut CircuitWires,
        _input: &Self::Input,
        _output: &Self::Output,
    ) {
        panic!("display pair FIFO is Verilog-only")
    }

    fn verilog_source() -> Option<String> {
        Some(include_str!("display_pair_fifo.v").to_string())
    }

    fn verilog_testbench() -> Option<String> {
        Some(include_str!("display_pair_fifo_tb.v").to_string())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "explicit external simulation of the inferred pair memory"]
    fn pair_memory_runs_in_iverilog() {
        digital_design_hardware::verify_verilog_with_iverilog::<super::DisplayPairFifo>().unwrap();
    }
}
