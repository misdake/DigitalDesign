# Bloxorz presentation

This continuous CPU V3 application uses all33 imported Bloxorz stages,
ordinary GPU meshes and the resource renderer. The 400×240 pixel-mode Camera,
low atlas, wells, textured floors/player, switches and projected shadow follow
the Web scene. Native lighting, RGB565 quantization and the flat animated goal inlay on ordinary-floor neighborhoods remain documented differences.

RIGHT/LEFT/DOWN/UP roll and repeat after settlement; the most recently pressed
held direction wins. START restarts, TRIANGLE/R1 select the next stage, L1 the previous stage, and
Square undoes. Cross swaps split cubes; Circle toggles frame statistics. SELECT toggles the original dither background and solid #2c2620.
Outgoing rule/animation clocks continue behind a cached RTT veil colored
#142238. Out and incoming Hold/In each retain one completed 3D snapshot.
Ordinary frames replay a static geometry CB directly into the selected Back,
then draw dynamic content. Recording is reused until level, background, camera
or fragile visibility changes; it stores World and material snapshots through
the normal public API. `TargetBinding` retargets the sealed CB and its optional
background copy together. Static color/depth cache surfaces are not allocated.
The schedule and ownership are specified in
[transitions](/systems/cpu-v3-tang-nano-20k/examples/game_bloxorz/docs/transitions.md).
The CPU records dynamic geometry while the static CB renders. A fade capture
retires both 3D submissions before the overlay samples the snapshot.
A presented opaque endpoint gates reset; the presented transparent
endpoint starts entry. Locked-period edges are discarded and held directions
require release. Ordinary rendering always shows the current level; optional DRAW/MS values describe the previous completed frame. MS measures vblank return through the final GPU wait, with cold-start dashes until that interval is available.

```powershell
& systems/cpu-v3-tang-nano-20k/examples/game_bloxorz/build.ps1
& scripts/run-cargo.ps1 -Subcommand test -Label bloxorz-rtt -CargoArgs @('-p','cpu-v3-tang-nano-20k','--test','host','gpu_pipeline::bloxorz::')
```

The explicit `gpu_renderer_bloxorz` selector pins column layout and RGB565 column
layout ABI2; diagnostic command versions come from the canonical capture ABI
definitions rather than an application-local number. It checks actual RCC library
bodies with rustc and compares generated boot bytes against independent CB06 packaging. Products stay under this worktree's
`target/`. This does not change the default boot application or program a board;
matching `production` GPU and Display hardware is required.

See the [presentation documents](/systems/cpu-v3-tang-nano-20k/examples/game_bloxorz/docs/README.md) for phase ordering, snapshot source ownership, veil arithmetic and verification.

All33 levels,256-entry complete undo, nine bridge channels, immediate fragile-tile disappearance at crack start,
split/swap/merge, tipping/release, entry/goal motion and cached transitions are
connected. Special-tile marks use a deterministic128KiB RGB atlas, preserving the
necessary moving bodies. Inactive bridge hints use alpha60. The imported reference
leaves stage23 unverified; this does not claim all33 levels solvable or Web PBR
equivalence. CPU/GPU/Display correctness tests do not establish hardware FPS.

Background bytes and their transfer/Display contract are described with the
[asset](/systems/cpu-v3-tang-nano-20k/examples/game_bloxorz/assets/README.md).
The [ground shadow](/systems/cpu-v3-tang-nano-20k/examples/game_bloxorz/docs/shadow.md) follows the prepared player matrix through clipped unit-triangle Instances.
Static floors are generated into a stable heap package at stage load and drawn in material groups; moving and transparent objects retain their own paths. Static well components share native mesh groups. Entrance/goal parts retain
separate Models and stable pose/texture caches, with fifteen metadata words
allocated once on the heap. The caller retains a packed
two-word resting block key so unchanged poses avoid rebuilding source World and
normal matrices. Animation poses still update; Camera revisions update the shared
View independently. Prepared bridge, switch and pad parts retain World and use
the enclosing Pass camera when replayed.

Stage 1 uses the original 10x6 board; stage 2 uses the original 15x6 board and its
two independent switch/bridge channels.

Application Flash writes require the matching production FPGA configuration.
Load an audited SRAM image after writing the application. Cold power-on also
requires that configuration in Flash.

To compose a persistent pair, pass `-HardwareArtifactDirectory target/.../board`
to this recipe. It audits the existing
RTT fit against current sources, checks that the raw configuration binary is
byte-for-byte the audited FS bitstream, and uses the standard packer to produce
`complete-flash.bin`: FPGA configuration at 0 and the same independently checked
CB06 application/resources at 0x100000. `profile.json` binds all three hashes.
The recipe remains offline. Application-only output is suitable for later
updates only when the stored FPGA configuration already supports this profile;
an application write can return the board to its stored FPGA configuration.

The HUD uses the frozen native 16px Vonwaon ASCII bitmap described in
[asset provenance](/systems/cpu-v3-tang-nano-20k/examples/game_bloxorz/assets/README.md).
The generated `hud-font-atlas-1x1.png` decodes the actual Native UI lookup without
scaling or smoothing. A local TTF is needed only to deliberately regenerate the
frozen source, never for the normal build.
