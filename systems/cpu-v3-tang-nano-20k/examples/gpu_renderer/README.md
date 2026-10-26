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
