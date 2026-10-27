# Bloxorz-shaped resource caller

This is a bounded API usage fixture and a continuous calling example, not the
complete Bloxorz game or a migration of its legacy renderer. It imports the
existing pure `bloxorz_core.rs` through an explicit ordinary source directory.
The asset producer reads the existing switch/split fixtures and canonical floor/
box geometry; it never runs the old prepare/raster cmodel. The independent host
oracle uses the original `geometry::roll` in world Y-up coordinates. Current
Camera/Model normal preparation remains rotation-only.

The application holds one fixed Camera, two Models, four immutable Mesh batches,
one Material and two persistent numeric fields. Logical package ranges select
floor0, floor1, bridge and block; native directory addresses remain in the facade.
Floor batches share a Model at half scale. The centered unit block uses semantic
orientation, pivot and elapsed-time rolling poses. The fractional clock uses
655/65536 per millisecond (below one tick of Q16 error per millisecond), and a roll
commits the original integer rule after 100ms. Input is sampled once per Frame;
held packets do not create new button edges. RIGHT/LEFT/DOWN/UP roll, START resets,
and TRIANGLE switches between the two fixture levels and resets. Split-block
rendering and complete gameplay are outside this fixture.

Separate completed 3D Passes draw floor, visible bridge and block. A small finish
function owns the Frame: disconnect returns early after the completed 3D Passes,
so cleanup cancels the entire Frame without GPU submission. Connected Frames add
moves/connection UI and complete normally. Game code owns no command buffers,
color planes, dirty/generation flags or cache publication tickets.

```powershell
& systems/cpu-v3-tang-nano-20k/examples/gpu_renderer_bloxorz/build.ps1
& scripts/run-cargo.ps1 -Subcommand test -Label 'renderer-bloxorz-usage' -CargoArgs @('-p','cpu-v3-tang-nano-20k','--test','host','gpu_pipeline::bloxorz::')
```

The build selects `CPU_V3_BOOT_EXAMPLE=gpu_renderer_bloxorz`, exports the generated
OUT_DIR boot/application/resources into `target/gpu-renderer-bloxorz-boot`, and
checks byte identity with an independent CB06 repack. Its labels/output are
separate from the minimal and changing-frame examples. It does not synthesize,
program Flash or operate a board.

The bounded entry runs 22 ticks/20 submissions. Controlled host events supply
25ms ticks, unchanged/held input, two disconnects/reconnects, two rolls, reset and
level changes. Tests check actual compiled state against the original rule trace,
per-draw native matrices against independent float geometry, mesh order, UI and
A/B reuse. The production CPU/FPU fixed-port fixture checks every store to held
command/object/UI inputs, permitting only the Recorded Target relocation after
old GPU/Display retirement and before publication. Matched changed/steady frames
have identical floor+bridge+block draw work. Costs separate acquisition/input,
rules, pose, UI edit, geometry, UI recording and seal/advance/cancel windows;
SIGNAL marker overhead is included. These peers provide no cache/SDRAM, real GPU,
scanout, concurrent-system or board qualification.
