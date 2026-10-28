# Minimal resource renderer

The RCC application uses the production host-approved `gpu_pipeline` facade:
one Renderer, Camera, Mesh, Model, Material and TextField, with one Frame/Pass per
tick. It draws one two-sided triangle, displays the last observed successful
submission duration as `OBS`, and integrates the held left stick on every frame.
Motion uses the frame's elapsed-time snapshot, capped at 100 ms, and clamps the
camera position. No new controller sample is required to continue moving.

```powershell
& systems/cpu-v3-tang-nano-20k/examples/gpu_renderer/build.ps1
```

This selects `CPU_V3_BOOT_EXAMPLE=gpu_renderer` for child builds, enables explicit
renderer compilation, materializes the exact Cargo OUT_DIR artifacts, and checks
an independent CB06 repack. It does not synthesize, place/route or program a board.
The default board application is unchanged.

The packer produces one native meshlet/directory, a padded white 1x1 RGB565
texture, and a complete native UI atlas. The classic font source is read without
modification; this packer adds O/B for the OBS label. Allocation checks use the
system's sole GPU layout. Mesh metadata, every vertex/index, padding and atlas
slot255 are checked before emitting resources and generated RCC metadata.

`renderer_three_frames` is the bounded caller entry used by host verification;
`main` runs continuously. The host case executes this actual file and these
actual assets, checks native command/object/UI bytes and explicit final drain.
GPU/Display progress in that case is controlled; it does not qualify actual
rasterization, scanout, cache timing, physical serial input or board behavior.

The OBS interval includes final publication and delayed observation of GPU
completion. It is not pure GPU execution time. Startup/invalid measurements show
`OBS --.-`; valid sub-10-ms values omit the leading zero. See the
[resource API](/systems/cpu-v3-tang-nano-20k/docs/gpu-renderer.md).

## Explicit members

The explicit member caller is `rcc/members.rs`. Build it with
`build.ps1 -Members` (mutually exclusive with `-Changes`). It records two draws
through one mutable Pass, consumes Pass/Frame with `finish`, then explicitly
submits and waits for both GPU and Display retirement. The build checks the
actual library bodies with rustc before RCC lowering; the existing generated
boot/independent CB06 comparison is retained. The `graphics_member` integration
target executes its actual three-frame entry in row and column layouts with
bounded CPU steps. The board selector uses explicit column16 assets/layout.
Continuous mode emits an `M` byte after each 64 fully retired frames, and an `E`
on a submit/wait failure when UART is writable; these are demo heartbeats, not a
capture protocol or a GPU performance measurement. Controlled
retirement peers do not qualify rendered pixels or board timing; see the
[member API](/systems/cpu-v3-tang-nano-20k/docs/gpu-renderer.md#explicit-member-calls).

## Changing frames

```powershell
& systems/cpu-v3-tang-nano-20k/examples/gpu_renderer/build.ps1 -Changes
```

The selector `CPU_V3_BOOT_EXAMPLE=gpu_renderer_changes` builds `rcc/changes.rs`
with the same immutable assets, into a separate default output directory and
separate cargo log labels. `renderer_changed_frames` executes eleven frames;
its normal entry cycles through the semantic changes continuously. The first
3D Pass retains its Camera snapshot while setters prepare a later Pass. One Mesh
is reused by two Models; old draw inputs remain copied snapshots. Alpha drawing
keeps call order. A later UI Pass changes visibility and a five-digit value to
one/two digits without packed glyph manipulation.

Host tests check the actual stream and objects independently and journal actual
production-core memory stores/cycles under fixed one-cycle peers. They cover
completed sequential Passes, cancellation of a Frame after any abandoned Pass,
held UI pages and zero UI writes once both pages converge. The cost windows and
limits are described in the [API](/systems/cpu-v3-tang-nano-20k/docs/gpu-renderer.md#actual-changing-caller-and-cpu-cost-fixture).
They do not establish real cache/GPU/Display concurrency or board timing.
