//! Device4 and the three shared-memory masters of the production GPU.
//! The Rust circuit emulator has no GPU executor; launching there fails explicitly.

use digital_design_circuit::{CircuitWires, Wire, Wires};
use digital_design_hardware::{
    BsramBlocks, Hardware, Module, ModuleIo, SsramBits, TargetResourceRequest,
};

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

    fn target_resources() -> Vec<TargetResourceRequest> {
        // Fixed native layout: the complete leaf owns its explicit DPB/RAM16 cells.
        let mut memory = TargetResourceRequest::new(BsramBlocks::new(36));
        memory.component = "GpuNativeMemory";
        memory
            .resources
            .extend(TargetResourceRequest::new(SsramBits::new(257 * 64)).resources);
        vec![memory]
    }

    fn create_emu(_input: &Self::Input, _output: &Self::Output) -> Self::EmuState {}

    fn execute_emu(
        _state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        input: &Self::Input,
        output: &Self::Output,
    ) {
        let selected = sample_wires(&input.device_index, circuit) == 4;
        if selected
            && sample_wires(&input.device_channel, circuit) == 1
            && input.device_write_enable.get(circuit) != 0
        {
            panic!(
                "GPU execution requires the RTL harness; the circuit emulator has no GPU executor"
            );
        }
        let read_data = 0;
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
        let mut source = include_str!("gpu.v").to_owned();
        source.push_str(
            &gpu_v2::system::vliw_command::verilog(
                gpu_v2::framebuffer::ports::MaterializedSurface {
                    color_base_bytes: 0x100000,
                    depth_base_bytes: 0x140000,
                    width: 400,
                    height: 240,
                },
                gpu_v2::system::pixel::quad_ingress::rtl::Backend::Native,
            )
            .expect("Production GPU export"),
        );
        Some(source)
    }

    fn verilog_testbench() -> Option<String> {
        Some(include_str!("gpu_tb.v").to_owned())
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

#[cfg(test)]
mod tests {
    use digital_design_hardware::Module;
    #[test]
    fn production_device_uses_the_command_closure() {
        digital_design_hardware::VerilogProject::generate::<super::CpuV3Gpu>().unwrap();
        let source = super::CpuV3Gpu::verilog_source().unwrap();
        assert!(source.contains("gpu_v2_vliw_command core("));
        assert!(source.contains("module gpu_command_processor("));
        let cp = source
            .split("module gpu_command_processor(")
            .nth(1)
            .unwrap()
            .split("endmodule")
            .next()
            .unwrap();
        assert!(!cp.contains("result_ready"));
        assert!(!cp.contains("result_offset"));
    }
}
