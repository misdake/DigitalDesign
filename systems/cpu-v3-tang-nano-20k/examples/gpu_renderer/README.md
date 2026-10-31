# Minimal explicit GPU API caller

The main RCC application uses the approved `gpu_api` root. The application owns
Gpu, CommandBuffer, Camera, Model, Mesh, Material, RenderState, TextureView,
Color/Depth buffers, UI grids and Display. There is no Renderer or Frame wrapper.
It reads gamepad/time itself, clamps elapsed movement to 100 ms and the eye to
+/-20, and records one textured, lit triangle plus an eight-cell OBS overlay.
Native mesh/texture/atlas bytes remain the same validated boot assets.

```powershell
& systems/cpu-v3-tang-nano-20k/examples/gpu_renderer/build.ps1
```

`CPU_V3_BOOT_EXAMPLE=gpu_renderer` explicitly selects the new approved root and
checked frontend. A custom format-2 project uses the `gpu-api` directive instead
of `renderer`; the two are mutually exclusive. Generated OUT_DIR assets and the
independent CB06 repack must match. No second boot byte array is maintained.
The default board application and hardware are unchanged.

## Per-frame lifetime

Boot scanout starts at Color A. The first frame writes Back B without waiting
for an unrelated vblank. Subsequent frames first confirm the preceding swap and
old-read retirement, then reuse the old Front as the next Back. The application
updates Camera and the selected external UI grid, resets/re-records its CB,
sets Camera/State, draws, and ends Pass by scope. It submits by value, takes the
same CB back from wait, and requests the next swap. Exit drains both GPU and
Display before dropping descriptors. The two external UI pages alternate with
the Back buffers; there is no CPU grid mirror. Texture invalidation is explicit
once after the boot asset upload, rather than hidden in every Pass.

OBS shows the preceding completed submission interval including publication,
capture gating and delayed observation. A resumed diagnostic pause marks that
observation unavailable; application motion uses milliseconds excluding pauses.
The observation is not pure GPU execution time.
The first frame and saturated intervals show `OBS --.-`. Valid sub-10-ms values
omit the leading zero. Statistics and formatting are application policy. RCC's
current byte slices are immutable constants, so dynamic digits use single-cell
writes; no compiler extension or persistent text mirror is introduced.

The bounded host test uses the same source and packed assets. It redirects only
main's stop policy to `renderer_three_frames`, preserving main's static byte
initialization. It checks actual native commands, UI bytes, explicit GPU/Display
retirement, equal healthy output in both check profiles and static/frame/stack
budgets. Its progress peers do not qualify GPU pixels, scanout or timing. Captured
execution uses the actual C parser; compiling a boot image alone is not a captured
Draw/UI qualification. Board access is not part of these checks.

The alternative callers use the same explicit Gpu/CommandBuffer root.

## Explicit members

Build `rcc/members.rs` with `build.ps1 -Members` (mutually exclusive with
`-Changes`). Each frame records two draws in a scoped Pass, moves the CB into
Gpu, receives it from wait and explicitly confirms Display retirement. The
three-frame entry is `renderer_member_three_frames`. The application uses
column16 assets and the approved root's checked resource methods.

## Changing frames

```powershell
& systems/cpu-v3-tang-nano-20k/examples/gpu_renderer/build.ps1 -Changes
```

The selector `CPU_V3_BOOT_EXAMPLE=gpu_renderer_changes` builds `rcc/changes.rs`
with immutable assets and separate output/log names. `renderer_changed_frames`
runs eleven frames; the normal entry repeats those changes continuously.
Camera/State binding and draw arguments become snapshots. Model, material and
camera edits affect subsequent recording; the application owns the external UI
grid, its text formatting, command reset, submission and presentation order.

Caller, diagnostic timing and RTT behavior are specified in
[Explicit GPU application callers](/systems/cpu-v3-tang-nano-20k/docs/gpu-explicit-examples.md).
CPU completion peers establish software protocol behavior. Production GPU/MC
replay and system-level simulation have separate evidence and limits.
