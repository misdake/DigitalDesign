use digital_design_circuit::CircuitWires;
use digital_design_hardware_common::ResetController;
use digital_design_hardware_gowin::{
    run_gowin_project_cli, ErasedSpiFlashImage, GowinCliError, GowinModuleProject, Hardware,
    HardwareIdentity, Module, SpiFlashReader, TangNano20K, TangNano20KFlashInputs,
    TangNano20KFlashOutputs, VerilogDependency,
};

fn main() -> Result<(), GowinCliError> {
    run_gowin_project_cli(gowin_project(), "target/cpu_v3_boot_flash_readback_gowin")
}

mod generated_boot {
    #![allow(dead_code)]
    include!(concat!(env!("OUT_DIR"), "/boot_images.rs"));

    pub(super) fn flash_package() -> &'static [u8] {
        FLASH_PACKAGE
    }
}

/// Read-only board probe that repeatedly streams the complete CpuV3 boot
/// package from Flash byte 0x100000 as checksummed UART records.
#[derive(Hardware)]
#[hardware(namespace = "systems/cpu_v3_tang_nano_20k/boot_flash_readback")]
struct FlashReadbackProbe;

type FittedFlashReader = SpiFlashReader<ErasedSpiFlashImage, 8_388_608, 2>;
type BoardReset = ResetController<16>;

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
        let flash_package = generated_boot::flash_package();
        assert!(
            flash_package.len() <= usize::from(u16::MAX),
            "Flash readback UART records use 16-bit package offsets"
        );
        Some(
            include_str!("self_test.v")
                .replace(
                    "__FLASH_READER__",
                    &FittedFlashReader::verilog_identity().module_name(),
                )
                .replace(
                    "__RESET_CONTROLLER__",
                    &BoardReset::verilog_identity().module_name(),
                )
                .replace("__FLASH_PACKAGE_SIZE__", &flash_package.len().to_string()),
        )
    }

    fn verilog_dependencies() -> Vec<VerilogDependency> {
        vec![
            VerilogDependency::new::<FittedFlashReader>("u_flash"),
            VerilogDependency::new::<BoardReset>("u_reset"),
        ]
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
    fn package_stream_readback_in_verilog() {
        digital_design_hardware::verify_verilog_with_iverilog::<FlashReadbackProbe>().unwrap();
    }
}
