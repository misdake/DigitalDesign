# DualShock2 GPU UI diagnostic

This standalone RCC example uses the actual Device5 driver and GPU character
Pass. It leaves the UART-only [minimal test](/systems/cpu-v3-tang-nano-20k/examples/dualshock2/README.md)
and default boot selection unchanged. No 3D scene or game is included.

The400x240 screen (800x480 physical output) shows:

- Connection state, reply ID, and successful analog sample count. ID and sample
  count are hexadecimal; configuration replies never count as analog samples.
- Held buttons and accumulated press/release masks, all hexadecimal. Events
  since the preceding UI recording are consumed only after that recording.
- LX/LY and RX/RY in decimal000..255. Center is128. Positive X is right and
  positive Y is down. Disconnection clears buttons and uses neutral axes.
- Two64x64 stick panels with a border, center crosshair and green2x2 dot.
  Raw values quantize to32positions per axis (`raw>>3`, two pixels per step),
  mapping0..7 to0 and248..255 to62. Numeric values retain full precision.
- All16 button labels with gray/green indicators. Mapping, from bit0, is
  SEL,L3,R3,START,UP,RIGHT,DOWN,LEFT,L2,R2,L1,R1,TRI,CIR,CROSS,SQR.

## Build and runtime

From the repository root:

```powershell
& systems/cpu-v3-tang-nano-20k/examples/dualshock2_ui/build.ps1
```

The script selects `CPU_V3_BOOT_EXAMPLE=dualshock2_ui` for its child builds,
restores the prior environment, materializes the authoritative Cargo OUT_DIR
assets below `target/dualshock2-ui-boot`, independently repacks CB06 and compares
it byte for byte by SHA256. It also exports hardware for source comparison;
it does not synthesize, place/route, access the board or program Flash.

`tools/assets.py` generates an original geometric5x7 font expanded into8x16
cells, button icons, panel tiles and dot phases in one64KiB RGBA5551 atlas at
0x4b0000, including native odd-row pixel ordering. Generated compact label data
is part of `boot_resources`; no duplicate executable/Flash array is maintained.
The boot pack contains only code, actual static initialization and this atlas.

The CPU initializes a reusable CB, clears the non-Front plane, writes labels,
numbers, panel tiles and indicators directly into the final UI object, then
records two one-cell dot overlays. There is no full-screen CPU grid snapshot.
After the true GPU completion, it presents through the existing A/B ownership
API and waits for the actual Display handoff before reusing storage. Palette
values are raw display RGB565, selected by `gpu_init()`.

`dui_service` owns the shared microsecond clear boundary and collects events.
It runs before each recording and during GPU/present polling. Packet helpers
never clear the timer; protocol edges/guard remain autonomous. GPU and Display
waits use the independent millisecond clock with a2s deadline. Driver faults
halt with0xd500|stage and light both remaining LEDs. LED0 otherwise means
running; LED1 means connected analog input.

Use the existing DS2 full-system bitstream, with left-port CLK/CMD/DAT/ATT on
17/20/19/18 and only LED0/1 outputs. The old six-LED profile is unsuitable.
Pin orientation,3.3V/GND and real input checks are in the minimal test README.
The visual test replaces its software Flash package; it does not require new
RTL. Reuse a routed artifact only after comparing its complete exported source
and configuration identity. Programming and physical acceptance require user
authorization; building is offline.

## Bounded verification and preview

```powershell
& scripts/run-cargo.ps1 -Subcommand test -Label ds2-ui-functional -CargoArgs @(
  '-p','cpu-v3-tang-nano-20k','--test','dualshock2_ui','--','--nocapture')
python systems/cpu-v3-tang-nano-20k/examples/dualshock2_ui/tools/preview.py `
  --fixture target/ds2-ui-validation/layout/center.json `
  --atlas target/ds2-ui-validation/layout/atlas.bin `
  --output target/ds2-ui-validation/preview.png
```

The layout test executes compiled production panel code, verifies actual command
and object memory for connection/disconnection, axis endpoints, all button bits,
numeric masks and the two overlay ranges. The one-frame entry executes the
actual application with GPU/Display/Device5 control peers and explicitly advanced
host time, bounded to4million CPU steps. It checks one submission/presentation
and DS2 service during both waits. The peers do not render pixels or exchange
serial wire bits; independent existing Device5 RTL tests cover that protocol.
The preview decodes actual compiled UI records and native atlas bytes; it is
not a board photograph, RTL rendering test or per-pixel acceptance comparison.

Real controller/cable behavior, display output and cold boot remain board tests.
