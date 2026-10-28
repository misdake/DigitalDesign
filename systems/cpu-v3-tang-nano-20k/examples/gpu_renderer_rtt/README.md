# Fixed400x240 RTT demos

Both demos use the [borrowed-buffer API](/systems/cpu-v3-tang-nano-20k/docs/gpu-render-targets.md)
and ordinary drawing. One GPU job renders an RGB triangle into a preallocated
source. After successful retirement, another job samples it:

- main.rs: fullscreen nearest, matching all400x240 logical pixels.
- bilinear.rs: an ordinary translated/rotated/scaled mesh using bilinear.

Shared scene.rs uses member finish, explicit submit and wait. There is no
animation, CPU pixel copying/reordering, UI/postprocess unit, Bloxorz or gradient.
Each demo presents once and halts, leaving its image visible. The bounded
renderer_rtt_two_updates regression entry checks source reuse and capture epochs.

```powershell
& systems/cpu-v3-tang-nano-20k/examples/gpu_renderer_rtt/build.ps1
& systems/cpu-v3-tang-nano-20k/examples/gpu_renderer_rtt/build.ps1 -Bilinear
```

Selectors gpu_renderer_rtt and gpu_renderer_rtt_bilinear check actual approved
library bodies with rustc, pin Column16+command ABI2 and compare generated boot
bytes with independent CB06 packaging. Outputs stay in this worktree target.
Packaging does not program a board or qualify timing.

The shared column asset packer supplies a white POT texture, atlas, RGB meshlet
and UV meshlet. External reservations are color byte0x600000 and depth byte0x640000,
capacity262144 bytes each. Assets avoid both complete spans. Physical ownership
stays with the caller throughout asynchronous use.

The optional hardware profile is rtt400x240-column16; see the
[offline board recipe](/systems/cpu-v3-tang-nano-20k/examples/rtt400_board/README.md).
Only use an audited matching bitstream/firmware/assets combination after explicit
device assignment. Receipts distinguish CPU peers, native sampler/micro-scenes
and the selected full-pipeline replay backend.
