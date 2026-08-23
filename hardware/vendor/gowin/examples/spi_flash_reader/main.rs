use digital_design_circuit::{input_const, input_w_const, Wires};
use digital_design_hardware_gowin::{
    run_gowin_project_cli, GowinCliError, HardwareIdentity, Module, SpiFlashImage, SpiFlashReader,
    SpiFlashReaderInput, TangNano20K, TangNano20KFlashInputs, TangNano20KFlashOutputs,
    VerilogIdentity,
};

fn main() -> Result<(), GowinCliError> {
    run_gowin_project_cli(
        TangNano20K::flash_debug_uart_project::<FlashReaderDemo>("spi_flash_reader"),
        "target/spi_flash_reader_gowin",
    )
}

/// Bytes used only when this board-level example runs in the host emulator.
struct DemoImage;

impl SpiFlashImage for DemoImage {
    const BYTES: &'static [u8] = b"DigitalDesign SPI Flash";
}

struct FlashReaderDemo;

impl HardwareIdentity for FlashReaderDemo {
    const TARGET_RESOURCE_LEAF: bool = false;

    fn verilog_identity() -> VerilogIdentity {
        VerilogIdentity::new("FlashReaderDemo").namespace(["examples", "spi_flash_reader"])
    }
}

impl Module for FlashReaderDemo {
    type Input = TangNano20KFlashInputs;
    type Output = TangNano20KFlashOutputs;
    type EmuState = ();

    const USES_MAIN_CLOCK: bool = true;

    fn emu(input: &Self::Input) -> Self::Output {
        build(input)
    }

    fn nand(input: &Self::Input) -> Self::Output {
        build(input)
    }

    fn build_verilog(input: &Self::Input) -> Self::Output {
        build(input)
    }
}

fn build(input: &TangNano20KFlashInputs) -> TangNano20KFlashOutputs {
    let mut address = input_w_const::<24>(0);
    address.wires[0] = input.buttons.wires[1];
    let mut length = input_w_const::<24>(0);
    length.wires[4] = input_const(1);
    let flash = SpiFlashReader::<DemoImage>::hardware(&SpiFlashReaderInput {
        start: input.buttons.wires[0],
        address,
        length,
        data_ready: input_const(1),
        flash_miso: input.flash_miso,
    });

    TangNano20KFlashOutputs {
        // LED 0=ready, 1=data-valid, 2=done, 3=error, and 4..5 show data bits.
        leds: Wires {
            wires: [
                flash.ready,
                flash.data_valid,
                flash.done,
                flash.error,
                flash.data.wires[0],
                flash.data.wires[1],
            ],
        },
        uart_tx: input_const(1),
        flash_clk: flash.flash_clk,
        flash_cs_n: flash.flash_cs_n,
        flash_mosi: flash.flash_mosi,
    }
}
