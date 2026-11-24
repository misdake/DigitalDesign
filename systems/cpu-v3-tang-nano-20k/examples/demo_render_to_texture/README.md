# Render-to-texture demo

Both demos use the [borrowed-buffer API](/systems/cpu-v3-tang-nano-20k/docs/gpu-render-targets.md)
and ordinary drawing. One GPU job renders an RGB triangle into a preallocated
source. After successful retirement, another job samples it:

- main.rs: fullscreen nearest, matching all400x240 logical pixels.
- bilinear.rs: an ordinary translated/rotated/scaled mesh using bilinear.

Both stages use the shared resident World/View path. Each Pass selects its
camera and each Model supplies World; producer retirement and texture ownership
remain explicit. Use World/View-capable gateware for these rebuilt applications.

Shared scene.rs uses scoped Pass recording, explicit submit and wait. There is no
animation, CPU pixel copying/reordering, UI/postprocess unit, Bloxorz or gradient.
Each demo presents once and halts, leaving its image visible. The bounded
renderer_rtt_two_updates regression entry checks source reuse and capture epochs.

```powershell
& systems/cpu-v3-tang-nano-20k/examples/demo_render_to_texture/build.ps1
& systems/cpu-v3-tang-nano-20k/examples/demo_render_to_texture/build.ps1 -Bilinear
```

Selectors gpu_renderer_rtt and gpu_renderer_rtt_bilinear check actual approved
library bodies with rustc and pin column layout ABI2 and the current fixed-depth
command ABI8. The common application command exports and validates a fresh
snapshot. Input reconstruction remains an explicit qualification of the shared
encoder. Outputs stay in this worktree target.
Packaging does not program a board or qualify timing.

The scene registers its mesh directory once, using nonowning MeshPtr slices for
the producer triangle and consumer quad. Transforms and state use the public
value constructors. Both consumer recording and presentation use the same Back
pointer; A/B selection does not duplicate the drawing path.

The shared column asset packer supplies a white POT texture, atlas, RGB meshlet
and UV meshlet. External reservations are color byte0x600000 and depth byte0x640000,
capacity262144 bytes each. Assets avoid both complete spans. Physical ownership
stays with the caller throughout asynchronous use.

The production hardware includes fixed400x240 views; see the
[production board build instructions](/hardware/vendor/gowin/scripts/README.md).
Only use an audited matching bitstream/firmware/assets combination after explicit
device assignment. Receipts distinguish CPU peers, native sampler/micro-scenes
and the selected full-pipeline replay backend.

Both stages use ordinary `Gpu::submit` and retire before source reuse or Display
handoff. CPU/GPU simulators inspect submitted jobs; application code does not
contain capture requests or a special capture submission API.
