# Fitted SPI Flash reader

`SpiFlashReader<Image, CAPACITY_BYTES, HALF_PERIOD_CYCLES>` is the read-only
target leaf for a board-fitted standard SPI NOR Flash. It supports a streaming
command interface suitable for boot loaders and asset loaders:

```text
start + address + length
          |
          v
data_valid + data <-> data_ready
          |
          v
        done
```

A command is accepted only while `ready` is high. Response bytes remain stable
while `data_valid && !data_ready`; the physical SPI clock pauses low during
that backpressure. `done` pulses after the final byte is accepted. A zero-byte
command completes without selecting the Flash. A range that exceeds
`CAPACITY_BYTES` also completes immediately, asserts `error`, and never lowers
chip select. Addresses and lengths are bytes.

The initial implementation emits only the standard read-data command `03h`
with a 24-bit address. It has no write-enable, program, erase, status-write, or
quad-enable path. This deliberately makes accidental modification impossible
at the component boundary. A later writer should be a separate, explicitly
dangerous component and API.

`Image::BYTES` supplies host-emulator contents. Reads beyond that slice return
the NOR erased value `FF`; the image is not embedded in Verilog. Calling
`SpiFlashReader::hardware` selects this emulator locally and the physical SPI
implementation during project export. The component has no NAND expansion and
claims the complete fitted `SpiFlashDevice` once.

For Tang Nano 20K the default specialization is 8 MiB and the default
half-period is two 27-MHz clocks, producing a 6.75-MHz SPI clock. The
`TangNano20K::flash_debug_uart_project` binding connects MCLK/59, MCS_N/60,
MO/61, and MI/62 and adds `set_option -use_mspi_as_gpio 1` to the generated
Gowin build. The example is intentionally one source file:

```text
cargo run -p digital-design-hardware --example spi_flash_reader -- --build
```

The `11_spi_flash_read` hardware characterization project in the validation
workspace identified the fitted device as a Winbond 64-Mbit part (`EF 40 17`)
with a valid SFDP 1.5 table. It validated standard-SPI runtime reads, both ends
of the 8-MiB address range, the MSPI pin mapping, and SRAM-only programming.
The reusable component was subsequently synthesized and placed/routed with the
same target and pin configuration.

The reader does not reserve regions within the Flash. A cartridge packer must
lay out the FPGA configuration image and payload segments without overlap,
then pass only validated segment ranges to the loader. `CAPACITY_BYTES` is a
local fail-fast boundary; target inventory remains the authority for the
physical fitted-device capacity.
