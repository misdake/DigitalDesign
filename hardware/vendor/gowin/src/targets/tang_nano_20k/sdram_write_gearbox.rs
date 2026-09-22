use digital_design_circuit::{Wire, Wires};
use digital_design_hardware::{Hardware, Module, ModuleIo};

#[derive(Clone, ModuleIo)]
pub struct TangNano20KSdramWriteGearbox108M54MInput {
    pub logic_clk: Wire,
    pub controller_clk: Wire,
    pub reset: Wire,
    pub capture_valid: Wire,
    pub capture_data: Wires<64>,
    pub write_start: Wire,
    pub burst_length: Wires<5>,
}

#[derive(Clone, ModuleIo)]
pub struct TangNano20KSdramWriteGearbox108M54MOutput {
    pub controller_data: Wires<32>,
}

#[derive(Hardware)]
#[hardware(namespace = "targets/tang_nano_20k/sdram")]
pub struct TangNano20KSdramWriteGearbox108M54M;

impl Module for TangNano20KSdramWriteGearbox108M54M {
    type Input = TangNano20KSdramWriteGearbox108M54MInput;
    type Output = TangNano20KSdramWriteGearbox108M54MOutput;
    type EmuState = ();

    const USES_MAIN_CLOCK: bool = false;
    const EMU_AVAILABLE: bool = false;

    fn verilog_source() -> Option<String> {
        Some(include_str!("sdram/write_gearbox_108m_54m.v").to_string())
    }

    fn verilog_testbench() -> Option<String> {
        Some(include_str!("sdram/write_gearbox_108m_54m_tb.v").to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "explicit external simulation of the related-clock write gearbox"]
    fn variable_bursts_run_in_iverilog() {
        crate::verify_verilog_with_iverilog::<TangNano20KSdramWriteGearbox108M54M>().unwrap();
    }
}
