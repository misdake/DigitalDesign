# GPU demo components

This independent system example contains an immutable native room/texture/font
packer and CPU character-overlay helpers. It currently checks those components;
it does not yet submit a GPU frame or present one. No driver, hardware module,
boot selection or machine-code image is duplicated here.

## Files and use

- `main.rs`: bounded compiled-CPU checks and native font-layout checks.
- `rcc/overlay.rs`: a 375-word cell table and previous-completed-frame text.
- `assets/font-8x16.txt`: original 8x16 bitmap source, under the repository MIT license.
- `tools/assets.py`: reproducible static resource packer and native-record audit.
- [Frame statistics](/systems/cpu-v3-tang-nano-20k/examples/frame_stats/docs/components.md):
  the reused timestamp/formatting state from
  [system RCC](/systems/cpu-v3-tang-nano-20k/rcc/frame_stats.rs); there is no second formatter.
- [UI format](/ip/gpu-v2/docs/ui.md) and
  [GPU driver](/systems/cpu-v3-tang-nano-20k/docs/gpu-driver.md): the owning interfaces.

```powershell
cargo test -p cpu-v3-tang-nano-20k --example gpu_demo
python systems/cpu-v3-tang-nano-20k/examples/gpu_demo/tools/assets.py --output target/gpu-demo-assets
```

The packer needs Pillow and the repository's existing V/T oracle prerequisites.
Generated assets and audit output belong in the selected new output directory.
It writes no command buffer, instruction array or Flash image.

## CPU overlay

The 50x15 grid occupies exactly 375 u16 words (750 bytes). Each row contains
25 words; the low byte is the earlier cell. `demo_cell_set` bounds-checks the
cell index and glyph ID before touching memory and preserves the neighbor byte.
The caller supplies a full 375-word array. FF cells preserve the 3D background.

Row 1, column 2 contains `FRAME ----- MS` initially. `demo_overlay_begin` receives
the current wall-clock timestamp and whether the preceding full frame succeeded.
It reuses the four-word `frame_stats` state and a five-word temporary digit array.
The first boundary and any false completion flag show five dashes; successful
subsequent boundaries show the modular millisecond difference with leading zeros.
The caller must keep intervals below 65536 ms and mark errors/timeouts invalid.
This helper performs no device I/O and does not infer success from polling.
It uses shifts, masks and the existing subtraction-only decimal formatter.

## Static physical resources

| Resource | Byte range | Layout |
| --- | --- | --- |
| Font atlas | `0x4b0000..0x4c0000` | Full 64 KiB, ASCII IDs; unlisted/255 entries transparent |
| Room mesh | `0x4c0000..0x4c0f80` | One padded directory, six prefixed native meshlets |
| Room texture | `0x4c1000..0x4c3c00` | 64-square RGB565, complete seven-level tiled mip chain |

The atlas contains white opaque glyph pixels and transparent background pixels,
already ordered for the UI framebuffer banks. The 18 source glyphs cover the
label, digits and placeholder; the entire 256-entry allocation is materialized.
The room and original tile image come from the existing GPU scene producers.
The packer records source/content hashes, validates disjoint extents below 8 MiB,
and compares metadata/payload with the existing native V/T oracle. Three existing
camera poses are recorded as data; one scale_s is chosen to cover all three.
This check does not claim rendered pixels, cache/Display ownership, timing or
physical-board qualification.
