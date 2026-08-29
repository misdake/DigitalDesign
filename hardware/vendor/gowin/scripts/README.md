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
builds and audits the board-health, CPU, CPU/SDRAM, complete boot, and Flash-readback artifacts. Neither mode
programs a device. Each successful mode writes a small evidence record below
`target/hardware-validation` containing the commit, dirty-worktree flag, and completed steps.

Board interaction uses `run_board_validation.ps1`, which deliberately separates
read-only artifact checks, observation, and hardware mutation:

| Mode | Actions |
| --- | --- |
| `Audit` | Validate the existing manifest, generated sources, bitstream, timing, and resources. No hardware access. |
| `Observe` | Wait a bounded time for an already-running board's VCP, capture UART, and validate its protocol. No programming. |
| `Program` | Audit, optionally write either the boot package or a complete power-on Flash image, then program the audited SRAM bitstream exactly once. |
| `Full` | Perform `Program`, then bounded VCP wait, capture, and protocol validation. |

Supported profiles are the FPGA-alive `board-health` probe and the full CPU V3
`cpu-v3-system` system (two-stage flash boot plus the SDRAM and HDMI datapaths).
Every attempted run writes `target/board-validation/<profile>/<UTC>/evidence.json`,
including failure stage, source/bitstream fingerprints, SHA-256 hashes, commit and dirty state.
The runner never resets USB and never retries programming after a failure.

```powershell
# Safe offline check; this is the default mode.
powershell -ExecutionPolicy Bypass -File hardware/vendor/gowin/scripts/run_board_validation.ps1 `
    -Profile cpu-v3-system -Mode Audit

# Observe an image that is already running, without touching FPGA or Flash.
powershell -ExecutionPolicy Bypass -File hardware/vendor/gowin/scripts/run_board_validation.ps1 `
    -Profile board-health -Mode Observe -Port COM8

# Explicit complete boot run. Flash and SRAM are each programmed at most once.
powershell -ExecutionPolicy Bypass -File hardware/vendor/gowin/scripts/run_board_validation.ps1 `
    -Profile cpu-v3-system -Mode Full -Port COM8 -WriteBootFlash

# Persist both FPGA configuration and the boot package for cold power-on.
powershell -ExecutionPolicy Bypass -File hardware/vendor/gowin/scripts/run_board_validation.ps1 `
    -Profile cpu-v3-system -Mode Full -Port COM8 -WriteCompleteFlash
```

The runner captures through `capture_bl616_uart.ps1`. It enters the onboard
BL616 console with the documented timed escape, selects `uart`, and keeps that
same serial session open while capturing. Reopening the VCP after `choose uart`
is not equivalent on every BL616 firmware revision. At the end it returns to
the quiet BL616 console before closing the handle; it never resets or
re-enumerates USB.

All DDHT projects transmit 8N1 at 115200 baud (27 MHz designs use divider
233, 54 MHz designs use 468); pass `-Baud` only for nonstandard captures.

`check_uart_status.ps1` validates raw UART captures containing repeated
eight-byte DDHT status frames. It also decodes the CPU V3 boot ABI's ten-byte
`CV3B` frames into Stage0/Stage1, descriptor/manifest/DMA/entry/internal
category, code, and 16-bit detail. A valid boot-error frame always takes
precedence over apparent DDHT success and is reported as a DUT failure rather
than framing or baud corruption. The checker rejects stale captures, frames
for another test, and any reported failure. Because a raw serial capture can
drop a byte on the host side, torn DDHT frames are tolerated up to one percent
of the success count (at least one); beyond that the capture is rejected:

```powershell
powershell -ExecutionPolicy Bypass -File hardware/vendor/gowin/scripts/check_uart_status.ps1 `
    -Path target/example/capture.bin `
    -TestId 0x01 -MinimumSuccessFrames 2
```

Pass `-ResultPath target/example/uart-status.json` to retain the decoded counts,
reason, and structured boot errors even when validation exits unsuccessfully.
The board-validation runner always does this and embeds the result into its
evidence record.

DDHT status `0` is success and every other status is failure. Tests that need
additional error addresses, observed values, or replayable vectors should
introduce a new protocol version and extend the shared decoder rather than
growing a private script inside one example. Set `-MaximumAgeSeconds 0` only
when deliberately inspecting an archived capture.

The former `cpu-v3-flash-readback`, `cpu-v3-flash-diagnostics`, and
`cpu-v3-boot-dma` probes were consolidated into the full system and are no
longer separate builds; the boot package is now exercised through the
`cpu-v3-system` profile's Flash-writing paths.
Assigned test IDs:

| ID | Test |
| ---: | --- |
| `0x01` | Tang Nano 20K BSRAM shapes self-test |
| `0x03` | Tang Nano 20K fitted SDRAM burst/refresh self-test |
| `0x07` | CPU V3 full system two-stage flash boot (application reached) |
| `0x0a` | Tang Nano 20K board clock/button/UART transport health probe |

The former CPU V3 CPU-execution (`0x04`), SDRAM (`0x05`), boot-DMA (`0x06`),
system-control-UART (`0x08`), device-path (`0x09`), and the read-only/diagnostic
Flash probes were consolidated into the full `cpu-v3-system` system and are no
longer separate builds.

## Stable board bring-up

Build once, then use the runner above or these lower-level commands to program
the audited image without rerunning synthesis or place-and-route:

```powershell
cargo run -p digital-design-hardware-gowin --example board_health -- --build
cargo run -p digital-design-hardware-gowin --example board_health -- --check-existing
cargo run -p digital-design-hardware-gowin --example board_health -- --program-existing
powershell -ExecutionPolicy Bypass -File hardware/vendor/gowin/scripts/capture_bl616_uart.ps1 `
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

## HDMI physical-link bring-up

`hdmi_color_bars` is the stand-alone Tang Nano 20K HDMI gate. It generates
1280x720p60 video with an auditable TMDS encoder, four Gowin `OSER10`
serializers, and four fitted differential output buffers. It does not use the
CPU, SDRAM, SPI Flash, or the debug UART:

```powershell
cargo test -p digital-design-hardware-gowin --example hdmi_color_bars `
    timing_and_tmds_control_codes_decode_in_iverilog -- --ignored --nocapture
cargo run -p digital-design-hardware-gowin --example hdmi_color_bars -- --build
cargo run -p digital-design-hardware-gowin --example hdmi_color_bars -- --check-existing
cargo run -p digital-design-hardware-gowin --example hdmi_color_bars -- --program-existing
```

The last command writes only volatile FPGA SRAM. Button1 overlays a 32-pixel
white grid and Button2 selects a horizontal grayscale ramp. The six LEDs show
the synchronized buttons, frame heartbeat, video-reset release, and PLL lock.
