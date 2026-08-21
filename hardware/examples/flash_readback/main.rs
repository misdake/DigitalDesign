use digital_design_code::CircuitWires;
use digital_design_hardware::{
    run_gowin_project_cli, ErasedSpiFlashImage, GowinCliError, GowinModuleProject, Hardware,
    HardwareIdentity, Module, SpiFlashReader, TangNano20K, TangNano20KFlashInputs,
    TangNano20KFlashOutputs, VerilogDependency,
};

fn main() -> Result<(), GowinCliError> {
    run_gowin_project_cli(gowin_project(), "target/flash_readback_gowin")
}

/// Board probe reading back the G16 boot package magic at Flash 0x100000.
/// LEDs 1..4 mirror per-byte matches against `G16B`, LED 5 is done, LED 6 is
/// error.
#[derive(Hardware)]
#[hardware(namespace = "examples/flash_readback")]
struct FlashReadbackProbe;

type FittedFlashReader = SpiFlashReader<ErasedSpiFlashImage, 8_388_608, 2>;

impl Module for FlashReadbackProbe {
    type Input = TangNano20KFlashInputs;
    type Output = TangNano20KFlashOutputs;
    type EmuState = ();

    const USES_MAIN_CLOCK: bool = true;
    const EMU_AVAILABLE: bool = false;

    fn execute_emu(
        _state: &mut Self::EmuState,
        _circuit: &mut CircuitWires,
        _input: &Self::Input,
        _output: &Self::Output,
    ) {
        panic!("FlashReadbackProbe is a Verilog-only hardware probe")
    }

    fn verilog_source() -> Option<String> {
        Some(
            include_str!("self_test.v").replace(
                "__FLASH_READER__",
                &FittedFlashReader::verilog_identity().module_name(),
            ),
        )
    }

    fn verilog_dependencies() -> Vec<VerilogDependency> {
        vec![VerilogDependency::new::<FittedFlashReader>("u_flash")]
    }

    fn verilog_testbench() -> Option<String> {
        Some(include_str!("probe_testbench.v").to_string())
    }
}

fn gowin_project() -> GowinModuleProject<TangNano20K, FlashReadbackProbe> {
    TangNano20K::flash_debug_uart_project::<FlashReadbackProbe>("flash_readback_probe")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "explicit external simulator validation"]
    fn package_magic_readback_in_verilog() {
        digital_design_hardware::verify_verilog_with_iverilog::<FlashReadbackProbe>().unwrap();
    }
}
