---
id: bloxorz-pixel-diagnostics
status: implemented
last-verified: 2026-11-03
---

# Native pixel diagnostics

The bounded host tests use the actual RCC application, instruction CPU, functional
GPU and Display conversion. `web_first_two_stage_play_frames_use_functional_gpu_and_actual_display`
saves the two level views, level2 right-edge standing, a gradient view and a real
solution advanced to goal-drop. `optimization_rotation_colour_depth_diagnostic`
is an explicit ignored experiment; select `BLOXORZ_SHADOW_CASE=right-standing`
for the fixed level2 position(13,2), or `production` for its real+Z roll at110ms.
`BLOXORZ_SIM_OUTPUT_ROOT` keeps these products in this feature tree's `target/`.
Each submission's `gpu-before-N.bin` contains the complete physical input memory;
`qualification.json` records the command base, count and work bound. Capture-time
CPU steps and functional work are not hardware frame time.

`tools/pixel_trace.py` generates observation-only copies of the selected GPU
functional modules. Production GPU sources are never edited. It appends per-pixel
observers, retaining the numerical functions and original execution order. The
executable asserts every resulting physical halfword equals the public Executor
for each original submission. Every observed fragment also checks its expected
ROP result against the value after the actual triangle retires. Input memory is
read only; replay results and JSONL are written to the explicit output directory.

```powershell
python -B systems/cpu-v3-tang-nano-20k/examples/gpu_renderer_bloxorz/tools/pixel_trace.py `
  --root . --output target/pixel-diagnostic/source --deps target/build-baseline/release/deps
& target/pixel-diagnostic/source/pixel-trace.exe `
  target/pixel-diagnostic/input target/pixel-diagnostic/replay target/pixel-diagnostic/points.json `
  1> target/pixel-diagnostic/covers.jsonl 2> target/pixel-diagnostic/execution.log
```

Use a current dependency build from the same worktree and an existing JSON array
of integer[x,y] points. The generator records original/observed source hashes,
generated support hashes and exact compiler/rlib identities. It fails if its
expected insertion points or immutable constant sets do not match, rather than
silently interpreting an unsupported implementation. Both original depth and
the accepted depth-shift implementation are recognized; the global shift is
included in the record. A new run should use a new output directory to retain
prior inputs and failures.

Each cover records submission/command/draw/triangle identity, source indices,
transformed positions, attributes, edge transport, model uniform, interpolated
UV/depth/normal, depth comparison, material/texture/filter/mip/LOD, nonzero tap
coordinates/weights/physical byte address/RGB565 value, and actual color/depth
write. RGB-mode geometry has no texture taps. The tool records the hatch's
texture sampling separately from the plain well/rim. Clear/copy/UI commands keep
their real execution order; selected scene pixels are outside the HUD.

An exact ray/plane calculation from the same transformed vertices can separate
transport error from physical geometry. Its candidate faces must satisfy exact
coverage and the mesh's front-face contract; a nearer invisible back face is
not a valid visibility oracle. This diagnostic is finite evidence for its fixed
inputs, not proof of arbitrary poses, physical scanout, timing or the user's
photograph unless that image's pose and camera have independently been matched.

`pixel_trace_analyze.py covers.jsonl --output exact-depth.json` performs that
independent calculation for one opaque scene frame, including exact top-left
coverage. Its report includes every positive wrong-nearest separation and a
separate configurable0.005-world-unit summary. That summary is a diagnostic
threshold, not a visual acceptance tolerance or a mask on image differences.


## Production goal aperture diagnostic

The ignored `goal_carrier_surface_ownership_diagnostic` test exercises the actual
production application: level2 PLAY, GOAL_OPEN100ms, GOAL_DROP210ms and
GOAL_CLOSE100ms after real session transitions, plus PLAY on levels15/17 to
verify dynamic-neighbor fallback. Each case has a4-million instruction-step
limit. It checks actual loaded carrier mesh records and the session phase;
fallback cases must not load a carrier. `BLOXORZ_SIM_OUTPUT_ROOT` selects a fresh
output directory. `BLOXORZ_CARRIER_CASE_SET=closing-and-fallback` selects only the
last three cases when the first three already have matching qualification.

All48 aperture meshes use the ordinary native record producer. Asset inspection
can independently sum triangle areas against the outer square minus the hole,
verify front winding, monotone aperture extent, Y0 and no triangle crossing the
open center. Native quantization is1/128 of the aperture width. Geometry and
allocation checks do not establish arbitrary-pose raster precision.

Each captured case retains the actual program/resources, full before-submission
memory, physical color/depth planes, Display PPM and qualification metadata. The
observer above records any residual contact error without a mask or relaxed
comparison. Small positive nearest-surface errors remain declared numerical
limits; the diagnostic is not a zero-error/all33-level/physical timing claim.
The standalone `goal_carrier_probe.py` remains a fixed-pose research asset tool;
normal builds and this production test use `assets.py` and `goal_carrier.py`.


## Heap batches and CPU cost

`load_time_ground_meshlets_preserve_all_legacy_grid_vertices_and_attributes`
runs the actual RCC builder across all33 levels with a40-million instruction
bound. Its independent reference decodes the old wire-format vertices before
applying each original tile's world translation. It checks source positions,
packed normals/UVs, triangle indices, material ordering and legal record sizes.
`same_heap_address_rebuilds_only_after_retirement_and_capture_reads_new_records`
runs levels1->2->1, verifies the two old CBs are reset and the GPU idle at each
rebuild, and invokes an explicit public capture hook. Header/vertex samples must
match current physical heap words; the returned level must restore the first
image. Its manifest is a test identity, not a board-verified build identity.

The ignored CPU-cost test in `gpu_renderer_bloxorz_batch_cost.rs` compares a
stationary stage8 for three frames, using the formal default hook and identical
resources/camera/input. A single diagnostic mode-initialization word selects the
old per-face path or heap batches. It separately records first-frame/cold entry,
load-generation markers and steady CPU/MC logic cycles, actual submitted DRAW,
meshlet, SET_RENDER and UI counts. Native GPU-start heap bytes and all final
color/depth/Display planes are checked against instruction simulation of the
same package. The native core's observed SP must stay above the configured heap.
The CPU bound is24M logic cycles and240 host seconds per arm; GPU execution is a
synchronous functional callback and vblank is advanced immediately by the host.
These CPU costs exclude GPU/Display bus competition, SPI loading and scanout
latency and must not be reported as hardware FPS or GPU speedup. Test markers do
not appear in the ordinary application.
