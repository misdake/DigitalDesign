# Classic rotating GPU scene

The reusable scene is the original brick-textured box, complete 948-face lit
rabbit and RGB triangle: 24 meshlets and1141 triangles. The audited256-pose
radius2.8 orbit retains its original pitch and rotating light. Room and RGB
are unlit; rabbit uses the original full-H9 diffuse/specular settings.
The texture is room-tiles.png with its real mip chain, not diagnostic mip colors.

## Files

- `rcc/main.rs`: formal driver recording, 3D/UI, completion and A/B presentation.
- `rcc/camera.rs`: bounded read of immutable matrix/light data.
- `rcc/overlay.rs`: packed cells and previous-frame decimal number.
- `rcc/controls.rs`: button0/pin88 toggles numeric visibility after20ms stable level; hold does not repeat, default visible.
- `assets/scene.json`: native geometry and camera/light asset data from the
  existing audited fullscene producer, with source provenance hashes.
  It contains no commands, CPU instructions or Flash image.
- `assets/font-8x16.txt`: original font under the repository MIT license.
- `tools/assets.py`: the single native resource producer.
- `main.rs`: bounded packed-cell/font component checks, including0/9/10 formatting.
- `tools/preview.py`: optional conversion of captured framebuffer words.
- `build.ps1`: OUT_DIR export and independent CB06 repacking.

## Runtime and layout

Each frame reads45 words containing21 Q16.16 matrix values and three signed
light values through the existing bounded DSEG3 window. It records room,
RGB triangle, then rabbit with matching material transitions, followed by UI.
One CB is reused after actual GPU completion, presentation and old-read drain.
The shared driver, hardware and default boot configuration are unchanged.

Only the preceding successful full-loop millisecond number appears at row1,
column2, with no leading zeros or FRAME/MS labels. Zero is one `0`; invalid and
first intervals are blank. All five prior cells are cleared before shorter text
is written. Existing DDGD UART telemetry, failure retention and2000ms polling
budgets remain unchanged. Timing includes CPU preparation, GPU and present.

| Allocation | Physical byte base | Contents |
| --- | --- | --- |
| atlas.bin |0x4b0000|Complete64KiB native atlas|
| room.bin |0x4c0000|Full scene directory/records plus256 camera records|
| texture.bin |0x4d0000|Original64-square brick image and full mip chain|

Directory entries0..5 are room, entry6 is RGB and entries7..23 are all rabbit
meshlets. Each record retains the audited full-orbit scale_s. Camera records
occupy128 bytes each; manifest/generated roots own the exact lengths.

## Build and verification boundary

```powershell
& systems/cpu-v3-tang-nano-20k/examples/gpu_demo/build.ps1
```

The script temporarily selects CPU_V3_BOOT_EXAMPLE=gpu_demo and restores its
previous value. Application/resources originate in Cargo OUT_DIR and are
exported by cpu-v3-boot-assets. Independent cpu-v3-pack reconstruction must
match CB06 SHA-256. The fixed loader uses Flash0x100000; Maintainer composes the
accepted FPGA configuration with this package for persistent programming.

The predecessor room regression is retained independently: frozen application,
camera, overlay, font and asset producer live in
`tests/fixtures/gpu_demo_room/`. The original `gpu_demo_native.rs`,
`gpu_demo_tb.v`, `gpu_demo_camera.rs`, `tools/gold.py` and `support.rs` now bind
to that fixture, not the changing production scene. Generic driver/UI/cache,
present and frame-statistics coverage also remains in its own suites.
Future routine checking can use one face and two rabbit meshlets instead of
full-scene/two-frame scans. For this revision the user explicitly waived all
functional tests and requested direct programming after build/package. No new
simulation, frame comparison, hardware fit or board qualification is claimed.

## Button-controlled numeric overlay

The fitted board exposes synchronized buttons through Device0 RECV8. A stable
low-to-high transition of bit0 toggles the timing overlay; releasing only rearms
it. The timer, camera and rendering continue while hidden. The next recorded
frame omits UI_GRID when hidden. No CPU/GPU reset is tied to this input.
