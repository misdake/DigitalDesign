# Hardware development scripts

This directory contains host-side tools shared by multiple hardware modules
and board examples. Example-specific RTL and test vectors remain beside their
owning crate's examples; reusable capture, decode, build, and reporting tools
belong here.

Repository-wide offline validation is orchestrated by `scripts/validate-hardware.ps1`:

```powershell
powershell -ExecutionPolicy Bypass -File scripts/validate-hardware.ps1 -Mode quick
powershell -ExecutionPolicy Bypass -File scripts/validate-hardware.ps1 -Mode iverilog
powershell -ExecutionPolicy Bypass -File scripts/validate-hardware.ps1 -Mode audit
powershell -ExecutionPolicy Bypass -File scripts/validate-hardware.ps1 -Mode pnr
```

`audit` checks those four existing artifacts against current sources without rebuilding. `pnr`
builds and audits the board-health, CPU, CPU/SDRAM, and complete boot artifacts. Neither mode
programs a device. Each successful mode writes a small evidence record below
`target/hardware-validation` containing the commit, dirty-worktree flag, and completed steps.

Board interaction uses `run_board_validation.ps1`, which deliberately separates
read-only artifact checks, observation, and hardware mutation:

| Mode | Actions |
| --- | --- |
| `Audit` | Validate the existing manifest, generated sources, bitstream, timing, and resources. No hardware access. |
| `Observe` | Wait a bounded time for an already-running board's VCP, capture UART, and validate DDHT. No programming. |
| `Program` | Audit, optionally write the boot package, then program the audited SRAM bitstream exactly once. |
| `Full` | Perform `Program`, then bounded VCP wait, capture, and DDHT validation. |

Supported profiles are `board-health`, `cpu-v3-cpu`, `cpu-v3-sdram`, and
`cpu-v3-boot`. Every attempted run writes `target/board-validation/<profile>/<UTC>/evidence.json`,
including failure stage, source/bitstream fingerprints, SHA-256 hashes, commit and dirty state.
The runner never resets USB and never retries programming after a failure.

```powershell
# Safe offline check; this is the default mode.
powershell -ExecutionPolicy Bypass -File hardware/vendor/gowin/scripts/run_board_validation.ps1 `
    -Profile cpu-v3-boot -Mode Audit

# Observe an image that is already running, without touching FPGA or Flash.
powershell -ExecutionPolicy Bypass -File hardware/vendor/gowin/scripts/run_board_validation.ps1 `
    -Profile board-health -Mode Observe -Port COM8

# Explicit complete boot run. Flash and SRAM are each programmed at most once.
powershell -ExecutionPolicy Bypass -File hardware/vendor/gowin/scripts/run_board_validation.ps1 `
    -Profile cpu-v3-boot -Mode Full -Port COM8 -WriteBootFlash
```

`capture_uart.ps1` records raw bytes from the board's debug UART (the Tang
Nano 20K exposes it as a USB serial port through the onboard debugger) into a
capture file:

```powershell
powershell -ExecutionPolicy Bypass -File hardware/vendor/gowin/scripts/capture_uart.ps1 `
    -Port COM8 -Out target/example/capture.bin
```

All DDHT projects transmit 8N1 at 115200 baud (27 MHz designs use divider
233, 54 MHz designs use 468); pass `-Baud` only for nonstandard captures.

`check_uart_status.ps1` validates a raw UART capture containing repeated
eight-byte status frames. A frame contains the `DDHT` magic, protocol version,
test ID, result, and XOR checksum. The checker rejects stale captures, frames
for another test, and any reported failure. Because a raw serial capture can
drop a byte on the host side, torn frames are tolerated up to one percent of
the success count (at least one); beyond that the capture is rejected:

```powershell
powershell -ExecutionPolicy Bypass -File hardware/vendor/gowin/scripts/check_uart_status.ps1 `
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
| `0x04` | CPU V3 compiled-program CPU/BSRAM execution self-test |
| `0x05` | CPU V3 boot BSRAM to SDRAM to instruction-cache execution self-test |
| `0x06` | Boot DMA flash-to-SDRAM engine self-test |
| `0x07` | CPU V3 two-stage flash boot (application reached) |
| `0x08` | System control device UART characterization (sysctl_uart) |
| `0x09` | CPU V3 CPU MMIO path characterization (cpu_v3_mmio) |
| `0x0a` | Tang Nano 20K board clock/button/UART transport health probe |

## Stable board bring-up

Build once, then use the runner above or these lower-level commands to program
the audited image without rerunning synthesis or place-and-route:

```powershell
cargo run -p digital-design-hardware-gowin --example board_health -- --build
cargo run -p digital-design-hardware-gowin --example board_health -- --check-existing
cargo run -p digital-design-hardware-gowin --example board_health -- --program-existing
powershell -ExecutionPolicy Bypass -File hardware/vendor/gowin/scripts/capture_uart.ps1 `
    -Port COM8 -Out target/board_health_gowin/capture.bin
powershell -ExecutionPolicy Bypass -File hardware/vendor/gowin/scripts/check_uart_status.ps1 `
    -Path target/board_health_gowin/capture.bin -TestId 0x0a -MinimumSuccessFrames 2
```

`--build` writes `gowin-build.manifest` beside the generated project. The
`--program-existing` path rejects a changed generated source set, target,
device, bitstream length, or bitstream fingerprint, then reruns timing and
physical-resource audits before invoking Programmer. This keeps a USB retry
from silently changing the FPGA implementation under test.

The board-health probe is the required first gate for higher-level hardware
tests. Its LEDs are: LED1 heartbeat, LED2/LED3 synchronized button levels,
LED4 completed-frame toggle, LED5 UART busy, and LED6 fabric-alive. A high
button is also returned as the DDHT status byte, so reset inputs remain
observable instead of silencing the probe. Do not interpret CPU, memory, or
boot results unless this probe first passes with the same physical setup.

The `sdram_word_port` example predates this protocol and still sends a
private `SDWP` frame; it is not validated by `check_uart_status.ps1`.
