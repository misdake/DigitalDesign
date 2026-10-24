# DS2 first-person GPU scene

The reusable scene is the original brick-textured box, complete 948-face lit
rabbit and RGB triangle: 24 meshlets and1141 triangles. The initial camera is
the original radius2.8 orbit phase0. Position, orientation, normals and light
then follow the autonomous DS2 input; the scene no longer advances an orbit. Room and RGB
are unlit; rabbit uses the original full-H9 diffuse/specular settings.
The texture is room-tiles.png with its real mip chain, not diagnostic mip colors.

## Files

- `rcc/main.rs`: formal driver recording, 3D/UI, completion and A/B presentation.
- `rcc/camera.rs`: initial classic pose, Prepared/FPU stick control and matching matrix/light updates.
- `rcc/overlay_gpu.rs`: previous-frame decimal number generated in the final UI object.
- `rcc/overlay.rs`: packed-grid helper retained for component tests and template diagnostics.
- `rcc/controls.rs`: button0/pin88 toggles numeric visibility after20ms stable level; hold does not repeat, default visible.
- `assets/scene.json`: native geometry and camera/light asset data from the
  existing audited fullscene producer, with source provenance hashes.
  It contains no commands, CPU instructions or Flash image.
- `assets/font-8x16.txt`: original font under the repository MIT license.
- `tools/assets.py`: the single native resource producer.
- `main.rs`: bounded packed-cell/font component checks, including0/9/10 formatting.
- `tools/preview.py`: optional conversion of captured framebuffer words.
- `build.ps1`: OUT_DIR export and independent CB06 repacking.

## Runtime and layout

Initialization reads the original45-word phase0 matrix/light record through
the bounded DSEG3 window. Stationary input preserves its exact codes. Each
frame reads one coherent latest DS2 snapshot and updates the camera only when
needed, using native Q16 FPU arithmetic. It records room,
RGB triangle, then rabbit with matching material transitions, followed by UI.
One CB is reused after actual GPU completion, presentation and old-read drain.
The default boot selection is unchanged. The demo explicitly requests linear
RGB565 scanout through gpu_init_with_format, retaining the original scene
linear-to-screen conversion. Raw RGB565 bypasses that conversion and appears
darker with the same framebuffer colors; mesh colors and lighting are unchanged.

Left stick moves horizontally relative to yaw, independent of pitch. L1 moves
world-down and R1 world-up; together they cancel. Right stick turns right/left
and looks up/down. Both sticks share the default Prepared calibration and radial
dead zone, without a Q8 output conversion or automatic recentering. Disconnection
stops motion; digital-only input supplies buttons and neutral analog axes.
Movement is2 world units/s and rotation3 radians/s, using the independent wall
millisecond delta including presentation waits. Updates cap dt at100ms; yaw
wraps at +/-pi, pitch clamps to +/-1.5rad, and each position component clamps to
+/-16 to stay inside the existing affine representation. These are numerical
limits, not scene collision. The classic basis is preserved: position uses
right/down/forward while normals and the camera-space light use right/up/back.
The F14 light is checked/repaired within the existing unit-length contract.

The preceding successfully presented frame's rendering interval appears at
row1,column2 as `0.0` through `65.5`ms, truncated to0.1ms without extra leading
zeros or FRAME/MS labels. First, failed and saturated intervals emit no UI call.
The application clears Device0 channel11 before input/camera/CB preparation and
reads it after actual GPU completion/drain, before calling present. Thus the
interval includes CPU preparation/upload and GPU work but excludes present,
vblank and subsequent UART reporting. A65535us reading is invalid, not wrapped.
Only successful presentation promotes that locked interval to the next display.

The demo sets its final UI object and draws only the visible glyph range
`[52,52+glyphs)`, initializing only intersecting packed words. Each frame's
3D redraw removes old text. Descriptor reuse and button visibility remain.
DDGD telemetry version3 reports each newly completed frame's locked rendering
interval after presentation, in raw microseconds;
packet length/checksum/status/completed count are unchanged;65535 denotes
an overrange interval and is not a valid displayed duration. The2000ms
polling budgets and20ms button debounce retain the independent millisecond
clock. No full scene or image comparison is required for timer validation.

| Allocation | Physical byte base | Contents |
| --- | --- | --- |
| atlas.bin |0x4b0000|Complete64KiB native atlas|
| room.bin |0x4c0000|Full scene directory/records plus256 camera records|
| texture.bin |0x4d0000|Original64-square brick image and full mip chain|

Directory entries0..5 are room, entry6 is RGB and entries7..23 are all rabbit
meshlets. Each record retains the audited full-orbit scale_s. The original256
camera records remain immutable for provenance, but live rendering loads only
phase0 at initialization. Records occupy128 bytes; generated roots own lengths.

## Build and verification boundary

```powershell
& systems/cpu-v3-tang-nano-20k/examples/gpu_demo/build.ps1
```

The script temporarily selects CPU_V3_BOOT_EXAMPLE=gpu_demo and restores its
previous value. Application/resources originate in Cargo OUT_DIR and are
exported by cpu-v3-boot-assets. Independent cpu-v3-pack reconstruction must
match CB06 SHA-256. The fixed loader uses Flash0x100000; Maintainer composes the
accepted FPGA configuration with this package for persistent programming.

The predecessor room regression is retained independently: frozen application,
camera, overlay, font and asset producer live in
`tests/fixtures/gpu_demo_room/`. The original `gpu_demo_native.rs`,
`gpu_demo_tb.v`, `gpu_demo_camera.rs`, `tools/gold.py` and `support.rs` now bind
to that fixture, not the changing production scene. Generic driver/UI/cache,
present and frame-statistics coverage also remains in its own suites.
Future routine checking can use one face and two rabbit meshlets instead of
full-scene/two-frame scans. For this revision the user explicitly waived all
functional tests and requested direct programming after build/package. No new
simulation, frame comparison, hardware fit or board qualification is claimed.


## Button and one-frame profiling

The fitted board exposes synchronized buttons through Device0 RECV8. A stable
low-to-high transition of bit0 toggles the timing overlay; releasing only rearms
it. The timer, camera and rendering continue while hidden. The next recorded
frame omits UI_GRID when hidden. No CPU/GPU reset is tied to this input.

`tests/gpu_demo_profile.rs` runs one production classic-scene frame through real
CPU/D-cache/GPU/Display RTL and the production arbiter. It records CPU stage
wall cycles/retired16-bit words, CP command spans and each meshlet's V/T enabled
steps and wall time. It has a12-million-cycle/3600-second watchdog and does not
compute a gold image or compare framebuffer samples. Its instruction ROM and
controlled64-bit memory peer omit instruction-cache misses and SDRAM pin timing;
54MHz logic and33.3MHz pixels do not make that peer a board performance model.
UART uses the shorter test divider; boot and full instruction fetch are outside this profile.
Frontend meshlet spans and backend work can overlap; they must not be summed as
sequential whole-frame time. The profiling setup seeds a17ms previous-interval text fixture, without a warm-up
render, so the numeric UI path is exercised. The stop policy ends after the one
actual presentation and old-read drain, omitting the legacy full-scan tail.

For native-memory timing, `GPU_DEMO_NATIVE_PROFILE=1` exports the same compiler's
single-wall/RGB/two-rabbit-meshlet program and assets, with the numeric UI enabled.
Run `python systems/cpu-v3-tang-nano-20k/tests/gpu_stream_frame.py` with `legacy`,
`stream-old-cache`, or `stream-cache` to compare the actual arbiter, CL3 bridge,
controller and delayed pin model. Each variant uses identical program/assets;
the committed cache is the baseline. The native profile ends at the driver's
real GPU-complete marker after checking four draws, one UI and all write
terminals. It measures CPU preparation/cache maintenance, command spans,
meshlet V/T and overlapping frontend/backend activity. Concurrent Display reads
retain priority and ownership checks; a subsequent display-boundary wait is
outside the rendering interval. The 12-million-cycle/1200-second bounds remain
explicit. Instruction fetch is direct ROM and boot is excluded; this is an RTL
memory-service comparison, not a board FPS measurement or an image oracle.

Setting `GPU_DEMO_CPU_PREP_PROFILE=1` runs the same ignored test only through
CPU submission, with a 200000-cycle limit. It requires the accepted frame's
`profile/program.hex`, checks exact program identity, and writes separate
`cpu-prep/` artifacts. Falling-edge PC/state residency and rising-edge cache
handshakes separate scalar recording, window copying and cache maintenance.
PC residency includes stalls, and function boundaries can shift a few samples;
data request/response states include ordinary cache-hit latency. This mode
does not render another frame or modify the deployed image.

For a software candidate, also set `GPU_DEMO_PROFILE_VARIANT` to a short name
containing only letters, digits or hyphens. Its `cpu-prep-<name>/` output retains
the exact accepted RTL while allowing new software. Bounded CPU-only recording
of both old and new images must produce identical complete command/object
arenas, and the candidate RTL's post-barrier memory dump must match those bytes.
The control peer never executes GPU commands; only the real RTL prefix supplies
cycle measurements. Original frame/preparation artifacts remain intact.

## Retained preparation and two-input-slot experiment

`rcc/template_experiment.rs` is a diagnostic program loaded only by the profiling
fixture. It does not replace the production driver, boot selection or demo.
Set `GPU_DEMO_CPU_PREP_PROFILE=1`, a unique `GPU_DEMO_PROFILE_VARIANT`, and
`GPU_DEMO_PREPARE_MODE` to select one of these bounded experiments:

- `retained`: build the classic scene once, capture/rebase a second input slot,
  publish initialization, then measure the next pose's button/text/UART work,
  dynamic input patching and actual submission barrier. No frame is rendered.
- `pingpong`: one wall meshlet, RGB and two rabbit meshlets, with two actual GPU
  submissions. CPU prepares the alternate input slot during GPU execution.
  Output B is explicitly discarded after completion; Front A stays protected.
- `pingpong-present`: the same compact submissions, with an actual first B swap
  before rendering the second result into released A. The second completed
  output is discarded, avoiding a second Display scan wait.

The two command slots are 0x490000/0x498000 and object slots are
0x4a0000/0x4a8000, each 32 KiB inside the original fixed allocations. The
experiment retains command order and static snapshots, patching three Instance
matrix prefixes, light XYZ and three packed digit words. Template initialization
uses the formal builder; its DMA_LOCAL sources and UI grid identity are rebased
for slot1. Generic topology/material/UI visibility changes require rebuilding
the template. The previous-time text remains the fixture's synthetic 17 ms.

Only Free/Completed input slots may be prepared. GPU submission still waits
for Idle and a confirmed Writable output target, then completes cache handoff.
Preparing CB data does not reserve or modify its future output plane. The
one-frame-ahead policy prepares n+1 while GPU executes n; only after submitting
n+1 does CPU reuse the completed input slot for n+2. GPU executes one CB at a
time; physical memory bandwidth and Display frame boundaries remain shared.

The fixture compares retained inputs against a fresh formal-builder recording
at the next pose. Real RTL dumps both submission inputs, rejects any CPU write
to the currently executing input slot, checks actual GPU write terminals, and
preserves Front/old-read protection. Prefix and pipeline limits are 200000 and
2000000 cycles respectively; CPU-only reference execution is also bounded.
Preparation and full-frame profiles share the `gpu_demo_profile` test
executable. Run these experiments serially when rebuilding it on Windows.
No image oracle is generated or compared by these experiments.

## Direct command and object recording

The application now uses the formal driver's [direct high-memory recording](/systems/cpu-v3-tang-nano-20k/docs/gpu-driver.md#direct-high-memory-recording).
Commands and Target/Global/Render/Instance objects are generated at their final
addresses. Meshlet command combinations are appended as batches with a local
cursor; END padding is zeroed directly. UI uses the driver's begin/write/end
interface to fill its final object and write only the visible decimal glyphs.
There is no complete low-memory cell table or grid snapshot in this demo.
The legacy packed-grid snapshot API remains available to other callers. The
fixed capacity and full validation/ordering contract remain unchanged.

Use `GPU_DEMO_CPU_PREP_PROFILE=1` and a unique `GPU_DEMO_PROFILE_VARIANT` with the
ignored `--test gpu_demo_profile` target for a submission-prefix measurement.
There is no alternate builder module or `GPU_DEMO_DIRECT_BUILDER` selection.
`gpu_demo_direct_fault` checks the accepted unrecoverable halt/fault policy and
zero GPU starts after injected memory errors. Its source-identity test matches
the final driver against the completed ui-inplace-v1 timing instructions. These
prefix/fault checks do not render or compare a complete frame.

## Bounded first-person checks

`cargo test --release -p cpu-v3-tang-nano-20k --test gpu_demo_first_person`
executes the production RCC camera against an independent floating reference,
checks controls/disconnection/dt/limits, and runs the actual frame body with
small asset counts and control peers. It deliberately varies presentation wait,
GPU delay, failure and saturation. These peers do not render pixels or claim
serial/pin timing; the separate GPU/DS2 RTL qualifications own those boundaries.
