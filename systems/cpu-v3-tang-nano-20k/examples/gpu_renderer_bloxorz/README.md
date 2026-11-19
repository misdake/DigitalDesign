# Bloxorz RTT presentation

This continuous CPU V3 application uses all33 imported Bloxorz stages,
ordinary GPU meshes and the resource renderer. The 400×240 pixel-mode Camera,
low atlas, wells, textured floors/player, switches and projected shadow follow
the Web scene. Native lighting, RGB565 quantization and the flat animated goal inlay on ordinary-floor neighborhoods remain documented differences.

RIGHT/LEFT/DOWN/UP roll and repeat after settlement; the most recently pressed
held direction wins. START restarts, TRIANGLE/R1 select the next stage, L1 the previous stage, and
Square undoes. Cross swaps split cubes; Circle toggles frame statistics. SELECT toggles the original dither background and solid #2c2620.
Outgoing rule/animation clocks continue behind a cached RTT veil colored
#142238. Out and incoming Hold/In each retain one completed 3D snapshot; ordinary
frames restore independent static color/depth into Back before drawing dynamic
content. Cache invalidation follows level, background, camera and fragile
visibility changes; the transfer schedule and storage are specified in
[transitions](/systems/cpu-v3-tang-nano-20k/examples/gpu_renderer_bloxorz/docs/transitions.md).
The frame loop advances gameplay, refreshes the static scene when needed,
records dynamic geometry while the color/depth copy runs, then records the
overlay and presents. A fade capture retires its 3D snapshot before the overlay
can sample it.
A presented opaque endpoint gates reset; the presented transparent
endpoint starts entry. Locked-period edges are discarded and held directions
require release. Ordinary rendering always shows the current level; optional DRAW/MS values describe the previous completed frame. MS measures vblank return through the final GPU wait, with cold-start dashes until that interval is available.

```powershell
& systems/cpu-v3-tang-nano-20k/examples/gpu_renderer_bloxorz/build.ps1
& scripts/run-cargo.ps1 -Subcommand test -Label bloxorz-rtt -CargoArgs @('-p','cpu-v3-tang-nano-20k','--test','host','gpu_pipeline::bloxorz::')
```

The explicit `gpu_renderer_bloxorz` selector pins column layout and RGB565 column
layout ABI2; diagnostic command versions come from the canonical capture ABI
definitions rather than an application-local number. It checks actual RCC library
bodies with rustc and compares generated boot bytes against independent CB06 packaging. Products stay under this worktree's
`target/`. This does not change the default boot application or program a board;
matching `production` GPU and Display hardware is required.

See the [presentation documents](/systems/cpu-v3-tang-nano-20k/examples/gpu_renderer_bloxorz/docs/README.md) for phase ordering, snapshot source ownership, veil arithmetic and verification.

All33 levels,256-entry complete undo, nine bridge channels, immediate fragile-tile disappearance at crack start,
split/swap/merge, tipping/release, entry/goal motion and cached transitions are
connected. Special-tile marks use a deterministic128KiB RGB atlas, preserving the
necessary moving bodies. Inactive bridge hints use alpha60. The imported reference
leaves stage23 unverified; this does not claim all33 levels solvable or Web PBR
equivalence. CPU/GPU/Display correctness tests do not establish hardware FPS.

Background bytes and their transfer/Display contract are described with the
[asset](/systems/cpu-v3-tang-nano-20k/examples/gpu_renderer_bloxorz/assets/README.md).
The [ground shadow](/systems/cpu-v3-tang-nano-20k/examples/gpu_renderer_bloxorz/docs/shadow.md) follows the prepared player matrix through clipped unit-triangle Instances.
Static floors are generated into a stable heap package at stage load and drawn in material groups; moving and transparent objects retain their own paths. Static well components share native mesh groups. Entrance/goal parts retain
separate Models and stable pose/texture caches, with fifteen metadata words
allocated once on the heap. The caller retains a packed
two-word resting block key so unchanged poses preserve the driver's existing derived
matrix cache. Animation poses still update, and Camera revisions independently
invalidate that driver cache.

For unattended capture-card diagnosis, build with `-AutomaticCapture`. Add
`-CaptureBrightness 255` for the full-brightness comparison; the default is 128. This
explicit selector runs entry, holds PLAY for 2s of clamped application time,
makes one real +X move, holds the result for 2s, then restarts and halts only
after the selected RTT transmittance consumer has completed WAIT_ALL. Display keeps scanning
the last framebuffer. It ignores physical input and does not invoke the PC
profiler. UART progress is ASCII: stage plus four hexadecimal digits and LF;
A=loop number, B=source begin, C=source retired, D=consumer submit,
E=consumer wait, F=presented mode, H=selected-brightness halt. This diagnostic does not
change the ordinary application selector.

Stage 1 uses the original 10x6 board; stage 2 uses the original 15x6 board and its
two independent switch/bridge channels. The diagnostic's first +X move is a
real stage 1 move, rather than the former switch-fixture demonstration.

Application Flash writes do not install the matching FPGA configuration. Load
an audited `production` SRAM image after writing the application; cold
power-on additionally needs that configuration in Flash. A previous stored
FPGA profile cannot qualify this RTT application. The capture diagnostic draws
`1000 + tint` over the consumer (1128 or 1255 at the stop) and reports active Display
address/format/pending and GPU status after its selected-brightness halt marker.

`1128`, the halt marker and idle/completion status indicate software/protocol
progress only. They do not validate source pixels, texture addressing, color
results or HDMI output; corrupted frames can carry the same overlay. Bind UART
and pictures to the same programming interval before interpreting them together.

To compose a persistent pair, pass `-HardwareArtifactDirectory target/.../board`
to this recipe (optionally with `-AutomaticCapture`). It audits the existing
RTT fit against current sources, checks that the raw configuration binary is
byte-for-byte the audited FS bitstream, and uses the standard packer to produce
`complete-flash.bin`: FPGA configuration at 0 and the same independently checked
CB06 application/resources at 0x100000. `profile.json` binds all three hashes.
The recipe remains offline. Application-only output is suitable for later
updates only when the stored FPGA configuration already supports this profile;
an application write can return the board to its stored FPGA configuration.

The HUD uses the frozen native 16px Vonwaon ASCII bitmap described in
[asset provenance](/systems/cpu-v3-tang-nano-20k/examples/gpu_renderer_bloxorz/assets/README.md).
The generated `hud-font-atlas-1x1.png` decodes the actual Native UI lookup without
scaling or smoothing. A local TTF is needed only to deliberately regenerate the
frozen source, never for the normal build.
