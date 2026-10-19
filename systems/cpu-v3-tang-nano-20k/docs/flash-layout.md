# External Flash and cold-start workflow

| Flash byte range | Owner |
| --- | --- |
| `0x000000..0x0fffff` | FPGA configuration reserve |
| `0x100000..0x10001f` | Fixed single-application v5 header |
| `0x100020..0x10007f` | Alignment padding |
| `0x100080..` | Nonzero even application payload, at most 128 KiB |

The package ABI is defined once in [boot-image-format.md](boot-image-format.md).
`boot-applications.conf` selects `rcc/system-display-test.rs`, the
[CPU-only Display diagnostic](system-display-test.md). CSEG7/DSEG0 and SP=0xc000
remain the application layout. There is no alternate button-selected image.

Use `cpu-v3-boot-assets` to materialize the build-generated `application.v3bin`, package and map, then `cpu-v3-pack application.v3bin` to independently repack and compare bytes. The optional `--configuration-bin` packer argument produces a full erased-padded Flash image and rejects configuration overlap with the 1 MiB application base. The board-validation script consumes the generated application directly rather than a section manifest.

For an application-only update, program the generated package at `0x100000`. The FPGA loader logic is unchanged by application bytes or length. A new application starts on a cold FPGA configuration/power cycle; runtime buttons, a software reset command and the old repeatable global reset tree are absent. Reconfiguration may reuse the same audited FPGA image. Programming or observing the board is a separate authorized physical procedure; these source changes alone are not board evidence.

Declared FPGA register state and initialized RAM metadata define power-up. A monotonic synchronized PLL-ready latch releases only the native SDRAM controller's local cold reset; bridge transport and system modules use declaration initial state. A synchronized SDRAM-ready gate starts the fixed loader. The CPU remains held until complete successful code and static writes. Once released, buttons do not restart SDRAM, caches or the CPU. Loss of established clock lock requires reconfiguration and does not trigger a partial runtime restart.

During boot the LEDs show `000001`; a sticky hardware failure shows `11xxxx`
with the error codes in the format document. After release the application
owns LEDs and UART. The selected diagnostic writes through the CPU memory
path and reports DDHT `0x0b` only after an actual Display target switch;
it does not submit GPU commands.
