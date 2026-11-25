# Box and rabbit demo

The reusable scene is the original brick-textured box, complete 948-face lit
rabbit and RGB triangle: 24 meshlets and1141 triangles. The initial camera is
the original radius2.8 orbit phase0. Position, orientation, normals and light
then follow the autonomous DS2 input; the scene no longer advances an orbit. Room and RGB
are unlit; rabbit uses the original full-H9 diffuse/specular settings.
The texture is room-tiles.png with its real mip chain, not diagnostic mip colors.

## Build and run

```powershell
cargo run --release -p cpu-v3-tools --bin cpu-v3-app -- preview demo-box-rabbit
cargo run --release -p cpu-v3-tools --bin cpu-v3-app -- build demo-box-rabbit --out target/handheld
```

For unattended board stability tests, `--motion orbit` selects a continuous
512-frame camera orbit around the same box, rabbit and RGB geometry. It runs
without controller input and keeps the normal frame timing and UART reports:

```powershell
cargo run --release -p cpu-v3-tools --bin cpu-v3-app -- build demo-box-rabbit --motion orbit --out target/box-rabbit-orbit
```

`--motion slices` displays a rolling
window over the rabbit. The application records its 17 meshlets in six groups
once, draws three consecutive groups each frame, and advances the first group
every 16 successful presents. A window crossing the end uses two slices in the
same list. Each list also carries the normal room/triangle prefix and overlay
tail. The room supplies View and Target. Each rabbit group starts with its
World and material setup. The camera remains under the usual DS2 controls.

```powershell
cargo run --release -p cpu-v3-tools --bin cpu-v3-app -- build demo-box-rabbit --motion slices --out target/box-rabbit-slices
```

Use `--motion ring` to inspect the wrap from a fixed camera. A bright arc
travels over a dim circular track. The marker at
twelve o'clock identifies the seam. The arc spans 12 of 32 sections and advances
one section every eight presents. The whole ring is recorded once, with a
command offset for each section. Each frame selects one prepared slice or two
when the arc crosses the seam, and submits them in one command list with the
track and overlay. The selected slices replay the original section draws.

```powershell
cargo run --release -p cpu-v3-tools --bin cpu-v3-app -- build demo-box-rabbit --motion ring --out target/box-rabbit-ring
```

`rcc/public_main.rs` uses the checked GPU API and unified gamepad input.
`tools/public_assets.py` reuses the classic scene producer and publishes its
asset descriptors. The room and RGB geometry use an independent prepared part;
the rabbit and overlay are recorded each frame.
Public Mesh draws use resident World/View. The room/RGB part retains its World
data and commands; each frame appends it into a Pass with the current Camera
and target. No scene packet patching is required. Rabbit draws use the same
shared encoder.
The public caller registers the mesh directory once, selecting room/RGB/rabbit
through nonowning slices. Its control and frame-statistics buffers belong to the
run scope; only the asynchronous diagnostic packet remains shared by the wait
helpers. Native asset factories stay at the application compatibility boundary;
ordinary Camera, Pose, Material and RenderState setup uses SDK constructors.
Use matching World/View-capable gateware. The low-level fixtures described below
retain their original precomposed workload for independent comparisons.

## RGBA4444 example

Build the independent `rgba4444` application with the ordinary application command:

```powershell
cargo run --release -p cpu-v3-tools --bin cpu-v3-app -- build demo-rgba4444 --gpu-debug 0 --out target/rgba4444-example
```

`rcc/rgba_main.rs` uses the public API to draw the complete classic opaque scene,
then two straight-alpha panels with SrcOver, depth testing and no depth writes.
The same 128-square RGBA4444 texture has sixteen alpha bands, transparent circular
holes, three color bands and all eight box-filtered mip levels. Trilinear sampling
filters RGB and alpha together. The panel view explicitly selects ClampToEdge
at each mip, preserving transparent borders without opposite-edge wrap. Classic
periodic textures keep Repeat. The initial screen-left panel uses uniform alpha
128 and screen-right uses 255, exposing uniform-times-texture alpha. DS2 camera
controls are shared with `demo-box-rabbit`. That application's source and assets
remain unchanged. The example has 25 stored meshlets and 1145 submitted triangles
because the extra two-triangle quad is drawn twice.
`rcc/rgba_scene.rs` owns the persistent mesh, models, materials, textures/views and
the separate opaque/transparent states. The frame loop updates only Camera and
records draws; it does not recreate resource registrations or mutate state while
recording. MeshPtr slices select all five draws from one mesh owner.

`tools/rgba_assets.py` composes the original classic producer, appends the quad
to its mesh resource, and emits the separate column-packed alpha texture and
its generated roots. It records actual resource/source hashes and checks ranges
and overlap; no command, instruction or Flash bytes are hand-maintained. This
example requires the RGBA4444-capable gateware; the old RGB565-only gateware is
not compatible with its material encoding.

## Low-level regression fixture

The direct-recording scene under `rcc/main.rs` is retained for native,
column-layout and profiling tests. The following measurements and
recording details describe that test workload.

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
vblank and idle UART waiting. Nonblocking report work can overlap GPU waits;
there is no UART-ready spin inside a running frame. A65535us reading is invalid, not wrapped.
Only successful presentation promotes that locked interval to the next display.

The demo sets its final UI object and draws only the visible glyph range
`[52,52+glyphs)`, initializing only intersecting packed words. Each frame's
3D redraw removes old text. Descriptor reuse and button visibility remain.
DDGD telemetry version3 reports a successfully presented frame's locked rendering
interval in raw microseconds. Immediately after present returns, the next loop
reads input before preparing that report. Bytes are sent nonblocking, one at a
time, only while GPU/present remains pending. An unusually short frame or slow
UART preserves an in-flight complete packet and may omit a success report; the
reported frame count makes omissions observable. Error reporting drains the
previous packet and reliably sends the error before halting. Bounded test exit
may drain remaining reports. There is no report wait between present return and
the next input read. Packet length/checksum/status/completed count are unchanged;65535 denotes
an overrange interval and is not a valid displayed duration. The2000ms
polling budgets and20ms button debounce retain the independent millisecond
clock. No full scene or image comparison is required for timer validation.

| Allocation | Physical byte base | Contents |
| --- | --- | --- |
| atlas.bin |0x4b0000|Complete64KiB native atlas|
| room.bin |0x4c0000|Full scene directory/records plus256 camera records|
| texture.bin |0x4d0000|Original64-square brick image and full mip chain|

Directory entries0..5 are room, entry6 is RGB and entries7..23 are all rabbit
meshlets. Position results remain F16 without a per-meshlet shift; metadata
word1 bits3:0 are zero. The original256
camera records remain immutable for provenance, but live rendering loads only
phase0 at initialization. Records occupy128 bytes; generated roots own lengths.

## Button and one-frame profiling

The fitted board exposes synchronized buttons through Device0 RECV8. A stable
low-to-high transition of bit0 toggles the timing overlay; releasing only rearms
it. The timer, camera and rendering continue while hidden. The next recorded
frame omits UI_GRID when hidden. No CPU/GPU reset is tied to this input.

`tests/apps/gpu_demo_profile.rs` runs one production classic-scene frame through real
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

`cargo test --release -p cpu-v3-tang-nano-20k --test host gpu_demo_first_person::`
executes the production RCC camera against an independent floating reference,
checks controls/disconnection/dt/limits, and runs the actual frame body with
small asset counts and control peers. It deliberately varies presentation wait,
GPU delay, failure and saturation. These peers do not render pixels or claim
serial/pin timing; the separate GPU/DS2 RTL qualifications own those boundaries.
