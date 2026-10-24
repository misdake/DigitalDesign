# Standalone DS2 cold-boot test

This RCC application uses the actual CPU-system Device5 byte engine. It configures
analog mode and reports connection, ID, buttons, and four independent raw axes
on the system UART (115200 baud, 8N1). It does not submit GPU work or draw 3D.
LED0 means application running; LED1 means a valid analog snapshot is connected.
The existing cold-start Display remains virtual black until explicitly presented.

For an on-screen diagnostic with numeric axes, button indicators and two stick
plots, use the separate [GPU UI example](/systems/cpu-v3-tang-nano-20k/examples/dualshock2_ui/README.md).
This UART-only application remains the smaller bring-up option.

Run from the repository root:

```powershell
& systems/cpu-v3-tang-nano-20k/examples/dualshock2/build.ps1
```

This sets `CPU_V3_BOOT_EXAMPLE=dualshock2` only for its child Cargo commands,
restores the previous environment, materializes authoritative OUT_DIR artifacts
in `target/dualshock2-boot`, independently repacks using `boot.conf`, compares
SHA256 and exports `gateware/`. It does not run synthesis, PnR or programming.
The configured default application in `boot-applications.conf` is unchanged.
The same environment selects this application for a later approved full build.

The main artifact is `cpu-v3-boot.bin` (CB06), with `application.v3bin`, static
word count, map, source-derived resource constants and asset manifest. Do not
manually copy instruction or Flash arrays into source. Gateware must be built
from this new DS2 profile: the older six-LED bitstream drives DAT/pin19 and is
unsuitable. The Maintainer owns final combined synthesis, PnR, STA/SDF, resource
and image-identity audits. No physical qualification is claimed by export.

## Wiring

View the board component side, USB-C above and HDMI below. Left-side header
positions count from its top. Confirm adapter silkscreen and continuity rather
than relying on wire colors; plug/socket front views are mirrored.

| Signal | Left header position | FPGA package pin | DS2 nine-pin contact |
|---|---:|---:|---:|
| CLK output, idle high | 15 | 17 | 7 |
| CMD output | 16 | 20 | 2 |
| DAT input | 17 | 19 | 1 |
| ATT output, active low | 18 | 18 | 6 |
| Logic VCC | 19 | board 3.3V supply | 5 |
| Common GND | 20 | ground | 4 |

Leave vibration supply/contact3, unused8 and ACK9 unconnected. Use 3.3V logic
and DAT pull-up, never 5V at FPGA GPIO. Inspect the adapter for an existing
pull-up; choose any additional external resistor based on measured rise time
(the protocol references discuss 1–10k ohm). The target weak pull-up is enabled,
but is not a substitute for measuring actual cable/LED loading. LED2..5 remain
physical loads even though their output bindings are removed.

## User board procedure after Maintainer audit

1. Record source SHA, audited gateware SHA256, CB06 SHA256 and the final persistent
   Flash image SHA256. Use the existing programming and placement procedure in
   [Gowin board tools](/hardware/vendor/gowin/scripts/README.md); the CB06 image
   alone is an application package, not a complete FPGA configuration image.
2. With power off, verify orientation, ground and 3.3V connections. Connect one
   controller. Program only the Maintainer-audited combined DS2 hardware and
   application image, with programming verification.
3. Open the debug UART at 115200 8N1. Actually remove and restore board power;
   a JTAG reload is not evidence of persistent cold boot. Expect `DS2`, then a
   status line on connection changes and approximately every250ms.
4. Lines use hexadecimal fields, e.g. `C=0001 ID=0073 B=0000 RX=0080 RY=0080
   LX=0080 LY=0080`. C=0 invalidates input and reports neutral axes. B is the
   active-high 16-bit button mask. ID=0079 is also a valid analog reply.
5. Move right X, right Y, left X and left Y separately, sweep each to both ends,
   then release. Record actual ranges/centers; raw values can span0000..00FF.
   Confirm each reported axis changes independently, buttons press/release,
   and reconnect while buttons/sticks are held establishes a fresh event baseline.
6. Unplug while running, confirm C=0 and cleared buttons, reconnect and confirm
   configuration and all four axes recover. Repeat cold boot with controller
   absent, then attach it. Record controller/adapter model and wired/wireless
   behavior separately; clone compatibility is not assumed.
7. If available, inspect CLK125kHz, ATT, CMD and DAT with a scope/logic analyzer:
   confirm idle high, 4us half periods, >=128us byte guard, 3.3V levels and DAT
   settling. Keep UART capture and observed LEDs/cold-boot behavior with image IDs.

No board programming or physical observations have been performed for this
delivery. Electrical levels, LED loading, real controller configuration response,
wireless adapters, cable rise time, whole-board fit and timing remain physical
acceptance boundaries. GPU/game integration, including Bloxorz, is outside this
test.

## Focused simulations

```powershell
& scripts/run-cargo.ps1 -Subcommand test -Label ds2-serial -CargoArgs @(
  '-p','cpu-v3-tang-nano-20k','--test','dualshock2','--',
  '--ignored','--nocapture','--test-threads=1')
```

Evidence is written under `target/dualshock2-validation`. Each simulator has a
global cycle limit; CPU tests use the actual production byte engine and an
independent serial controller, plus real CPU/D-cache and wall-clock/watch logic.
The preserved sibling `examples/dualshock2_protocol` is an independent Python
research reference, not an implementation dependency or hardware proof.
