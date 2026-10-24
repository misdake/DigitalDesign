//! Device5: autonomous 54 MHz latest-snapshot DS2 serial engine.
use crate::{Hardware, HardwareIdentity, Module, ModuleIo};
use digital_design_circuit::{CircuitWires, Wire, Wires};
use digital_design_hardware::{SsramBits, TargetResourceRequest};

#[derive(Clone, ModuleIo)]
pub struct DualShock2Input {
    pub reset: Wire,
    pub frame_event: Wire,
    pub device_index: Wires<3>,
    pub device_channel: Wires<4>,
    pub device_read_enable: Wire,
    pub device_write_enable: Wire,
    pub device_write_data: Wires<16>,
    pub dat: Wire,
}
#[derive(Clone, ModuleIo)]
pub struct DualShock2Output {
    pub device_read_data: Wires<16>,
    pub ds2_clk: Wire,
    pub cmd: Wire,
    pub att: Wire,
}
#[derive(Hardware)]
#[hardware(namespace = "components/dualshock2", target_leaf)]
pub struct DualShock2;
impl Module for DualShock2 {
    type Input = DualShock2Input;
    type Output = DualShock2Output;
    type EmuState = ();
    const USES_MAIN_CLOCK: bool = true;
    const EMU_AVAILABLE: bool = false;
    fn target_resources() -> Vec<TargetResourceRequest> {
        // Two pages x8 bytes, mapped to two16x4 dual-port distributed RAM cells.
        vec![TargetResourceRequest::new(SsramBits::new(128))]
    }
    fn execute_emu(_: &mut (), _: &mut CircuitWires, _: &Self::Input, _: &Self::Output) {
        panic!("DS2 is verified against an independent serial peer in RTL")
    }
    fn verilog_source() -> Option<String> {
        Some(include_str!("dualshock2.v").to_owned())
    }
    fn verilog_testbench() -> Option<String> {
        Some(
            include_str!("../../../tests/dualshock2_engine_tb.v")
                .replace("__DS2__", &Self::verilog_identity().module_name()),
        )
    }
}
