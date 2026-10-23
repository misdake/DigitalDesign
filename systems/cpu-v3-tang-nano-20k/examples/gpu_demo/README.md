# Rotating CPU/GPU demo

This independent application continuously records the textured room, overlays
character cells, waits for actual GPU completion and presents alternating A/B
planes. A 512-step camera orbit advances after each successful presentation.
The label shows the preceding successful complete-loop time. It uses the shared
RCC GPU driver and timer; the default boot selection remains unchanged.

## Files and use

- `main.rs`: bounded compiled-CPU checks and native font-layout checks.
- `rcc/main.rs`: the continuous application and bounded verification entries.
- `rcc/camera.rs`: Q16.16 yaw matrices, focal200 and forward translation8.
- `rcc/overlay.rs`: a 375-word cell table and previous-completed-frame text.
- `assets/font-8x16.txt`: original 8x16 bitmap source, under the repository MIT license.
- `tools/assets.py`: reproducible static resource packer and native-record audit.
- `tools/gold.py`: independent V/T, interpolation, mip sampling and LESS-depth reference.
- `tools/preview.py`: conversion of captured actual framebuffer words to PNG.
- `build.ps1`: OUT_DIR materialization and independent CB06 repacking.
- `support.rs`: compiled-CPU orbit and executable-identity validation helpers.
- [Frame statistics](/systems/cpu-v3-tang-nano-20k/examples/frame_stats/docs/components.md):
  the reused timestamp/formatting state from
  [system RCC](/systems/cpu-v3-tang-nano-20k/rcc/frame_stats.rs); there is no second formatter.
- [UI format](/ip/gpu-v2/docs/ui.md) and
  [GPU driver](/systems/cpu-v3-tang-nano-20k/docs/gpu-driver.md): the owning interfaces.

```powershell
cargo test -p cpu-v3-tang-nano-20k --example gpu_demo
python systems/cpu-v3-tang-nano-20k/examples/gpu_demo/tools/assets.py --output target/gpu-demo-assets
& systems/cpu-v3-tang-nano-20k/examples/gpu_demo/build.ps1
```

The packer needs Pillow and the repository's existing V/T oracle prerequisites.
Generated assets and audit output belong in the selected output directory.
Repeating the packer rewrites its declared outputs without deleting other files.
It writes no command buffer, instruction array or Flash image.

## CPU overlay

The 50x15 grid occupies exactly 375 u16 words (750 bytes). Each row contains
25 words; the low byte is the earlier cell. `demo_cell_set` bounds-checks the
cell index and glyph ID before touching memory and preserves the neighbor byte.
The caller supplies a full 375-word array. FF cells preserve the 3D background.

Row 1, column 2 contains `FRAME ----- MS` initially. `demo_overlay_begin` receives
the current wall-clock timestamp and whether the preceding full frame succeeded.
It reuses the four-word `frame_stats` state and a five-word temporary digit array.
The first boundary and any false completion flag show five dashes; successful
subsequent boundaries show the modular millisecond difference with leading zeros.
The caller must keep intervals below 65536 ms and mark errors/timeouts invalid.
This helper performs no device I/O and does not infer success from polling.
It uses shifts, masks and the existing subtraction-only decimal formatter.

## Static physical resources

| Resource | Byte range | Layout |
| --- | --- | --- |
| Font atlas | `0x4b0000..0x4c0000` | Full 64 KiB, ASCII IDs; unlisted/255 entries transparent |
| Room mesh | `0x4c0000..0x4c0f80` | One padded directory, six prefixed native meshlets |
| Room texture | `0x4c1000..0x4c3c00` | 64-square RGB565, complete seven-level tiled mip chain |

The atlas contains white opaque glyph pixels and transparent background pixels,
already ordered for the UI framebuffer banks. The 18 source glyphs cover the
label, digits and placeholder; the entire 256-entry allocation is materialized.
The room and original tile image come from the existing GPU scene producers.
The packer records source/content hashes, validates disjoint extents below 8 MiB,
and compares metadata/payload with the existing native V/T oracle. Three existing
camera poses are recorded as data; one scale_s is chosen to cover all three.
The complete rotating-camera range also has a conservative bound: SINCOS
interpolation stays in[-1,1], so room X/Z in[-3,3] and translation8 give
absolute H.z at most14. Smaller focal-scaled X/Y and a rounding allowance fit
the selected scale_s3. The compiled-CPU probe checks all512 actual matrices and
their native V/T output range. Static packing alone does not qualify pixels or
physical hardware.

## Frame ownership and timing

One reusable CB owns its command and object arenas. Each loop samples the real
millisecond counter, updates the fixed-width label, computes the camera matrix,
records a clear/3D/UI Pass, submits it and waits for actual GPU completion.
Presentation then waits for the Display swap and old-read drain before allowing
the next CB recording. The mesh, mip chain and complete atlas remain immutable.
The material is unlit UV with trilinear filter2, LESS depth and depth writes.
UI uses the private packed snapshot and does not modify Depth.

The first frame displays `FRAME ----- MS`. Following frames display the previous
successful loop-start interval, including CPU preparation, UART reporting, GPU
execution and presentation wait. GPU and present polling each have a2000ms
wall-clock budget; gaps obey the shared timer's less-than65536ms contract.
Failure/timeout clears the statistics validity, reports an error and halts while
retaining driver ownership. It does not publish a failed frame as normal.

The UART emits11-byte packets: `44 44 47 44 01 status count_lo count_hi ms_lo
ms_hi xor`. XOR covers bytes4..9. Status0 marks the initial boundary, status1
reports the preceding successfully presented full loop, and status`80|code`
reports failure with zero count/ms. Device7 writes are diagnostic observation
markers in simulation and have no connected board endpoint. LEDs show the low
five bits of the successful presentation count; an error sets bit5.

## Build and Flash handoff

`build.ps1` temporarily selects `CPU_V3_BOOT_EXAMPLE=gpu_demo`, then restores the
caller's environment. The system build runs the single asset producer inside
Cargo OUT_DIR and generates the actual application, resources and CB06 image.
`cpu-v3-boot-assets` materializes those exact bytes, resource identity manifest
and `demo-repack.conf`; the script independently repacks the exported application
and resources through `cpu-v3-pack` and compares complete package SHA-256.
Without the explicit environment selection, the default project is unchanged.

The generated package begins at Flash0x100000 and uses the existing fixed loader.
FPGA configuration composition, persistent Flash programming and power-cycle
cold-start verification belong to the board handoff. This application does not
program the board or alter the accepted FPGA hardware.

## Bounded verification

```powershell
& scripts/run-cargo.ps1 -Subcommand test -Label gpu-demo-camera -CargoArgs @('-p','cpu-v3-tang-nano-20k','--test','gpu_demo_camera')
& scripts/run-cargo.ps1 -Subcommand test -Label gpu-demo-native -CargoArgs @('-p','cpu-v3-tang-nano-20k','--test','gpu_demo_native','real_rotating_demo_presents_one_complete_frame','--','--ignored','--nocapture','--test-threads=1')
python systems/cpu-v3-tang-nano-20k/examples/gpu_demo/tools/preview.py target/gpu-demo-native
```

The native entry executes the same complete frame body once, then waits for its
full visible scan and halts. It checks real CPU/cache/SystemControl/GPU/Display
execution through the production arbiter, actual write completions, color and
Depth against an independent reference, immutable assets and Front/old-read
protection. Depth comparisons explicitly reject unknown bits. The captured
`actual1.hex` produces the actual `actual1.png` preview; it is not a host render.
The second prepared reference pose and the512-step CPU range check do not claim
a second native frame. Previous UI two-frame scans and compiled timing/character
checks cover those already-qualified paths; continuous advancement and later
displayed milliseconds are also observable in the board UART/HDMI run.

The cache-fault entry verifies zero GPU starts/writes/presents, retained Building
state and error-only telemetry. The final evidence test recompiles the current
RCC entries and continuous main, re-exports all six RTL endpoints and compares
the native inputs plus the materialized application. Native memory is a
controlled64-bit service, not SDRAM pin simulation; instructions use direct ROM.
Cold blank startup is accelerated and pixel clock is twice logic for stress.
UART uses a shorter divider in this profile; its cycle counts are not board FPS.
The existing hardware fit/timing qualification is reused; this is a software
application change.
