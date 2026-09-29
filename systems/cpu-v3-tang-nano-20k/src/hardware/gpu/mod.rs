//! Retired GPU integration point. The GPU v2 implementation is developed in
//! the standalone cmodel in this directory's `sim/` crate.
//!
//! This leaf preserves the system wiring while explicitly rejecting the old
//! command ABI. It does not execute commands or issue memory requests.

use digital_design_circuit::{CircuitWires, Wire, Wires};
use digital_design_hardware::{Hardware, Module, ModuleIo};

#[derive(Clone, ModuleIo)]
pub struct CpuV3GpuInput {
    pub reset: Wire,
    pub device_index: Wires<3>,
    pub device_channel: Wires<4>,
    pub device_read_enable: Wire,
    pub device_write_enable: Wire,
    pub device_write_data: Wires<16>,
    pub gpu_ro_request_ready: Wire,
    pub gpu_ro_write_data_ready: Wire,
    pub gpu_ro_response_valid: Wire,
    pub gpu_ro_read_data: Wires<64>,
    pub gpu_ro_response_last: Wire,
    pub gpu_ro_error: Wire,
    pub gpu_fb_w_request_ready: Wire,
    pub gpu_fb_w_write_data_ready: Wire,
    pub gpu_fb_w_response_valid: Wire,
    pub gpu_fb_w_response_last: Wire,
    pub gpu_fb_w_error: Wire,
    pub gpu_fb_r_request_ready: Wire,
    pub gpu_fb_r_write_data_ready: Wire,
    pub gpu_fb_r_response_valid: Wire,
    pub gpu_fb_r_read_data: Wires<64>,
    pub gpu_fb_r_response_last: Wire,
    pub gpu_fb_r_error: Wire,
}

#[derive(Clone, ModuleIo)]
pub struct CpuV3GpuOutput {
    pub device_read_data: Wires<16>,
    pub gpu_ro_request_valid: Wire,
    pub gpu_ro_write: Wire,
    pub gpu_ro_address: Wires<22>,
    pub gpu_ro_line_count_minus_1: Wires<2>,
    pub gpu_ro_write_data: Wires<64>,
    pub gpu_fb_w_request_valid: Wire,
    pub gpu_fb_w_write: Wire,
    pub gpu_fb_w_address: Wires<22>,
    pub gpu_fb_w_line_count_minus_1: Wires<2>,
    pub gpu_fb_w_write_data: Wires<64>,
    pub gpu_fb_r_request_valid: Wire,
    pub gpu_fb_r_write: Wire,
    pub gpu_fb_r_address: Wires<22>,
    pub gpu_fb_r_line_count_minus_1: Wires<2>,
    pub gpu_fb_r_write_data: Wires<64>,
}

#[derive(Hardware)]
#[hardware(namespace = "systems/cpu_v3_tang_nano_20k/gpu", target_leaf)]
pub struct CpuV3Gpu;

impl Module for CpuV3Gpu {
    type Input = CpuV3GpuInput;
    type Output = CpuV3GpuOutput;
    type EmuState = ();

    const USES_MAIN_CLOCK: bool = true;

    fn create_emu(_input: &Self::Input, _output: &Self::Output) -> Self::EmuState {}

    fn execute_emu(
        _state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        input: &Self::Input,
        output: &Self::Output,
    ) {
        let selected = sample_wires(&input.device_index, circuit) == 4;
        let status = sample_wires(&input.device_channel, circuit) == 2;
        let read_data = if selected && status && input.device_read_enable.get(circuit) != 0 {
            4 // Legacy GPU_STATUS_SUBMIT_REJECTED.
        } else {
            0
        };
        output.drive(
            circuit,
            &CpuV3GpuOutputValue {
                device_read_data: read_data,
                gpu_ro_request_valid: false,
                gpu_ro_write: false,
                gpu_ro_address: 0,
                gpu_ro_line_count_minus_1: 0,
                gpu_ro_write_data: 0,
                gpu_fb_w_request_valid: false,
                gpu_fb_w_write: false,
                gpu_fb_w_address: 0,
                gpu_fb_w_line_count_minus_1: 0,
                gpu_fb_w_write_data: 0,
                gpu_fb_r_request_valid: false,
                gpu_fb_r_write: false,
                gpu_fb_r_address: 0,
                gpu_fb_r_line_count_minus_1: 0,
                gpu_fb_r_write_data: 0,
            },
        );
    }

    fn verilog_source() -> Option<String> {
        Some(include_str!("gpu.v").to_owned())
    }
}

fn sample_wires<const W: usize>(wires: &Wires<W>, circuit: &CircuitWires) -> u64 {
    wires
        .wires
        .iter()
        .enumerate()
        .fold(0, |bits, (index, wire)| {
            bits | (u64::from(wire.get(circuit) & 1) << index)
        })
}
