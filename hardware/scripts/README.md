# Hardware development scripts

This directory contains host-side tools shared by multiple hardware modules
and board examples. Example-specific RTL and test vectors remain under
`hardware/examples/`; reusable capture, decode, build, and reporting tools
belong here.

`capture_uart.ps1` records raw bytes from the board's debug UART (the Tang
Nano 20K exposes it as a USB serial port through the onboard debugger) into a
capture file:

```powershell
powershell -ExecutionPolicy Bypass -File hardware/scripts/capture_uart.ps1 `
    -Port COM8 -Out target/example/capture.bin
```

All DDHT projects transmit 8N1 at 115200 baud (27 MHz designs use divider
233, 54 MHz designs use 468); pass `-Baud` only for nonstandard captures.

`check_uart_status.ps1` validates a raw UART capture containing repeated
eight-byte status frames. A frame contains the `DDHT` magic, protocol version,
test ID, result, and XOR checksum. The checker rejects stale captures, frames
for another test, bad checksums, and any reported failure:

```powershell
powershell -ExecutionPolicy Bypass -File hardware/scripts/check_uart_status.ps1 `
    -Path target/example/capture.bin `
    -TestId 0x01 -MinimumSuccessFrames 2
```

Status `0` is success and every other status is failure. Tests that need error
addresses, observed values, or replayable vectors should introduce a new
protocol version and extend the shared decoder rather than growing a private
script inside one example. Set `-MaximumAgeSeconds 0` only when deliberately
inspecting an archived capture.

Assigned test IDs:

| ID | Test |
| ---: | --- |
| `0x01` | Tang Nano 20K BSRAM shapes self-test |
| `0x03` | Tang Nano 20K fitted SDRAM burst/refresh self-test |
| `0x04` | G16 compiled-program CPU/BSRAM execution self-test |
| `0x05` | G16 boot BSRAM to SDRAM to instruction-cache execution self-test |
| `0x06` | Boot DMA flash-to-SDRAM engine self-test |

The `sdram_word_port` example predates this protocol and still sends a
private `SDWP` frame; it is not validated by `check_uart_status.ps1`.
