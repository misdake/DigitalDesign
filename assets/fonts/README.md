# Project UI font

All current on-device ASCII text uses **VonwaonBitmap16pxLite 1.02** by Haoyu
Qiu, originally introduced by the Bloxorz HUD. The frozen printable ASCII
32..126 in `vonwaon-bitmap16-ascii.txt` is the single glyph source. Each glyph
has 16 binary row masks, bit 7 at the left, an 8-pixel advance and baseline 14.
Use the native 8×16 pixels without resizing, smoothing or synthetic bold.

`vonwaon-bitmap16-provenance.json` preserves the supplied font's identity,
author URL and license declaration, including the author's upstream-rights
note. Normal builds need neither the original TTF, OS fonts nor a download.
`freeze_vonwaon.py` is the optional extraction tool for the exact pinned TTF;
it requires Pillow and accepts `--ttf`, `--bitmap` and `--provenance`.

The system's [UI font packer](/systems/cpu-v3-tang-nano-20k/tools/ui_font.py)
owns Native UI RGBA5551 encoding and bank ordering. Applications own palette,
highlight colors, glyph subsets and icon allocation. ID 255 is always the
transparent skip entry. A subset must leave room for its app's icon IDs.
Font pixels are immutable build-time assets, with no runtime font rasterizer.

Historical frozen fixtures retain their original bytes. CJK-specific tests
retain their separate native CJK fixture because this frozen subset is ASCII.
Host report annotations and browser/editor typography are outside the device
font contract.

## Reuse a base asset

For new 20K applications, select only the printable ASCII characters actually
needed. `ui_font.base_asset(characters, extra_glyphs=0)` preserves native ASCII
IDs and returns the required atlas extent, not a mandatory 64-KiB image. Omitted
records within that extent are transparent. Additional slots after the highest
selected character are available for application icons via `write_glyph`.

```python
import ui_font
data = ui_font.base_asset('ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789 /->', extra_glyphs=2)
first_icon = len(data)//256 - 2
ui_font.write_glyph(data, first_icon, icon_rows, foreground=0x07ff)
glyph_count = len(data)//256
sector_count = len(data)//128
```

Use a 64-KiB-aligned base address with that generated sector/glyph count when
constructing `UiAtlas`. ID255 is skip, regardless of the loaded glyph count;
its record need not be present. Include `ui_font.SOURCES` in the producer's
source manifest. `preview(data)` decodes either a selected or full native atlas.
The explicit UI API supports partial extents. Retained legacy capture/atlas
contracts still reserve a full address window and use `atlas(rows)`.
