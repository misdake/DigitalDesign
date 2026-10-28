# Fixed 400x240 RTT board profile

This offline recipe selects `CPU_V3_GPU_PROFILE=rtt400x240-column16` in the
complete `cpu_v3_system` factory. GPU rendering, texture reads and HDMI fetches
all use the same column-major 16x16 macrotiles with a 16-macrotile row stride.
It includes CPU, caches, shared arbiter, native SDRAM controller, Display and
boot. The [view contract](/ip/gpu-v2/docs/framebuffer-texture-view.md) describes
the descriptor and sampling arithmetic.

```powershell
& systems/cpu-v3-tang-nano-20k/examples/rtt400_board/build.ps1 -Mode Export -OutputDirectory target/rtt/export
& systems/cpu-v3-tang-nano-20k/examples/rtt400_board/build.ps1 -Mode Build -OutputDirectory target/rtt/board
& systems/cpu-v3-tang-nano-20k/examples/rtt400_board/build.ps1 -Mode Audit -OutputDirectory target/rtt/board
```

Export/Build requires a fresh directory inside this worktree's `target/`.
Audit checks the selected source set, Gowin manifest, physical resources, timing
and bitstream identity. The label defaults to the output leaf plus mode; pass
`-Label` to distinguish additional invocations. SPI half-period is pinned to 2.
The script restores environment variables and never programs hardware or opens
serial/capture devices.

## Bind matching firmware

Build the two [RTT applications](/systems/cpu-v3-tang-nano-20k/examples/gpu_renderer_rtt/README.md)
with their own recipe. They use source color at 0x600000 and depth at 0x640000,
each with 262144 bytes of reserved capacity. Each physical plane has 204800
bytes of CLEAR storage, including 12800 bytes of macro padding; its remaining
57344 bytes are not cleared. Assets are packed into column 16x16 texture layout
at build time. No CPU pixel conversion or per-frame repacking is involved.

The hardware recipe's default boot application remains the CPU Display
diagnostic. `-BootExample gpu_renderer_rtt` or `gpu_renderer_rtt_bilinear` selects
one explicit matching application. Hardware profile, boot package and assets
are separate identities and must all be audited before programming. Setting a
profile does not constitute a successful board run.

The absent/`production` hardware profile retains the ordinary factory. Existing
`texture16x8-column16` selects the column layout without the RTT view extension;
unknown profile names fail export. Cargo tracks this compile-time selection.
