# Resource renderer API

The system RCC root `gpu_pipeline.rs` exposes lexical Renderer, Frame, Pass,
Camera, Model, Mesh, Material and TextField resources over the
[existing two-input scheduler](/systems/cpu-v3-tang-nano-20k/docs/gpu-input-pipeline.md).
It retains one recording, one sealed successor and one running GPU; there is no
second production scheduler. The native single-slot `gpu` facade remains a
separate compilation mode and cannot be mixed with this root.

## Compile and ownership boundary

A format2 boot project may contain the standalone line `renderer`. The build
then uses `compile_renderer_program`, always approving the actual shared
`rcc/gpu_pipeline.rs` canonical path. Omitting the root is an error. The root
names twelve direct helpers, including private `gpu_scene`; application imports
cannot expose their raw state/functions or construct/disarm resource identities.
The source loader reports the selected file's actual canonical origin. Local
root/helper overrides fail approval; they never inherit a shared identity.
Generated `boot_resources` has no filesystem origin or library authority.
`device_abi` remains an ordinary shared module, but renderer compilation pins it
to the system source, so an application-local override cannot redirect devices.
Native diagnostics retain the String loader. Approval is an explicit host mode,
not inferred from source text or recovered by falling back to native compilation.

`renderer_open` constructs the sole owner after initialization. A second open
halts/faults before another owner is constructed. `renderer_frame` borrows it;
`frame_pass` snapshots a Camera and borrows the Frame. The resource compiler
prevents parent use while a child is live, fabrication, copying, escaped handles,
post-consumption use and raw helper access. Forwarding borrowed resources uses
ordinary explicit mutable reborrows.

```mermaid
flowchart LR
  R[Renderer] --> F[Frame recording]
  F --> P[Pass and prepared camera snapshot]
  P -->|pass_complete| S[Sealed input after frame_complete]
  F -->|implicit cancellation| C[Discard recording lengths]
  S -->|target bind and final cache publication| G[Single GPU]
  G -->|successful terminal| U[Release input and UI page]
  G -->|color ready and present| D[Display Pending]
  D -->|actual retirement| W[Writable color]
```

`pass_complete` ends the Pass; it does not submit. An abandoned Pass invalidates
that Frame. `frame_complete` then either cancels an unfinished Frame or seals and
transfers a completed one to the scheduler before advancing. A failed transfer
consumes the Frame and freezes referenced state, with no cleanup retry.
Frame cleanup only cancels its recording and retains desired UI. Renderer cleanup
never submits/waits; outstanding work freezes instead of releasing live inputs.
Only explicit `renderer_close` performs a bounded drain. Waits capture controller
input but do not consume frame edges. There is no implicit wait in a destructor.

## Scene resources and snapshots

Camera, Model, Mesh, Material and TextField are independent CPU metadata owners;
long-lived objects do not keep Renderer borrowed. Each registry has four slots,
with no heap or public integer identities. Releasing a handle releases metadata
only; immutable boot assets and already copied GPU input remain owned separately.
`renderer_mesh` registers the generated package's one mesh; its directory/extents
are checked once. The private known-mesh draw path retains capacity, mapping,
range, snapshot and final-publication checks without repeatedly validating the
whole immutable directory.

| API | Inputs and effect |
| --- | --- |
| `renderer_camera` / `camera_pose` | Eye and Euler radians; world right/up/back, zero camera looks -Z |
| `camera_projection` | Focal 144..256, far strictly between0 and200 |
| `camera_light` | Nonzero world-space light, normalized and quantized once per relevant change |
| `renderer_model` / `model_pose` | Translation, Euler radians, scale per component0.5..2 |
| `renderer_material` | RGB0..255 and lit/unlit |
| `material_color` | RGBA0..255; alpha selects native blending/depth-write behavior |
| `material_lighting` | Ambient/directional0..256, shininess0..16 |
| `material_specular` | RGB0..255 |
| `pass_draw` | Copies prepared native Global/Render/Instance data into the open input |

Eye/translation components are bounded to +/-100, angles to +/-4 radians, and
asset AABB components to +/-100. Eight transformed AABB corners must fit signed
26-bit/F16 `[-512,512)`. Negative/mixed camera Z is admitted; the legacy
all-positive-Z helper is not used. Euler rotation is Rz*Ry*Rx. Position is
`K * V_pos * M`; normals are `V_normal * R_model`, with rotation only. Model
scale and projection never enter the normal transform.

Controlled setters compare actual values. Camera rotation/translation,
projection rays and normalized light have separate dependencies. Each Pass
captures the prepared view; later camera edits do not rewrite that snapshot.
Model angle/affine and final position/normal caches invalidate independently.
The shared Pass view is compared by values rather than wrapping revision tags,
so camera-slot reuse cannot falsely match an old model cache. Default focal200
produces native rays(-8192,-4915,8192); reciprocal is taken after scaling focal
by256 to retain Q16 precision. Light components are first scaled by powers of two
until the largest magnitude is in[0.5,1], avoiding squared-length underflow and
coarse reciprocal-square-root precision at large magnitudes. Quantized light is repaired within a bounded
iteration count against the existing exact unit-vector predicate.

## Text and frame observations

`renderer_text(renderer,column,row,width)` owns one nonoverlapping single-row
field in the50x15 canvas. Width is1..50 and must fit its row. The application sees
neither packed glyph words, GPU addresses nor dirty/version flags. Each of four
fields caches up to50 logical glyphs; the metadata/glyph buffers use224 low words.
`frame_number` replaces a field and erases its short tail. `text_visible` hides or
restores cached text; hidden edits retain logical text without touching desired
pixels. Identical writes do not invalidate the canvas. `pass_text` records a
visible range, while `pass_observed_time` formats/draws `OBS --.-` or tenths of a
millisecond. Field cleanup does not erase desired UI or mutate recorded/inflight
pages. Snapshot and page-reference retirement remain the scheduler's responsibility.

`frame_input`, `frame_tick_ms`, `frame_elapsed_ms`, `frame_elapsed_seconds` and
`frame_previous` return stable observations for that Frame. Input is captured and
consumed once at its beginning; further queries do not read hardware. Held motion
remains available without a new packet and is integrated each game tick.
Do not mix direct gamepad consumption into a renderer application.

`FrameStats` contains ready/sequence, prepare_valid/prepare_us,
observed_valid/submit_to_observed_done_us, queue_ms and span_ms. It identifies the
last successfully observed GPU/color transfer, potentially older than n-1 while
another submission runs. `submit_to_observed_done_us` includes final publication
and CPU observer delay, not pure GPU time or Display/vblank. No extra polls are
added for measurements. The shared microsecond counter is cleared only at open
and healthy observation after an open recording has sealed/cancelled. Every
microsecond interval requires matching clear epochs, ordered endpoints and
neither endpoint saturated at65535. Finalized preparation survives later clears;
fields are never combined across different sequences. Millisecond differences
wrap modulo65536 and require a true interval below65536ms.

## Verification boundary

Bounded host execution covers real source origins, library isolation, concrete
ownership misuse, duplicate open, transfer failure, cancellation/drain/freeze,
text visibility/tails, timestamp epochs/saturation/wrap, delayed observation,
held input and Camera/Model cache dependencies. Independent float references and
native byte oracles check prepared position/rotation/rays/light and commands.
The [minimal application](/systems/cpu-v3-tang-nano-20k/examples/gpu_renderer/README.md)
uses the actual producer and only semantic public calls; its generated OUT_DIR
boot image matches an independent repack. Controlled GPU/Display peers establish
software ownership/bytes, not rasterization, scanout, real cache contention or
physical board qualification.
