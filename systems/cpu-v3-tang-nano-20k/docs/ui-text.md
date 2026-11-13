# UI text helpers

`rcc/ui_text.rs` is an ordinary software library over the public `UiGrid` API.
It has no privileged driver access, retained state, allocation, GPU submission
or wait. Import `mod ui_text; use ui_text::*;` alongside `gpu_api`.

| Function | Output |
| --- | --- |
| `text_u16(grid, x, y, value)` | Minimal unsigned ASCII decimal, including `0` |
| `text_u16_field(grid, x, y, width, value)` | Left-aligned decimal, truncated to width; trailing cells become skip255 |
| `text_i16(grid, x, y, value)` | Signed ASCII decimal, including `-32768` |
| `text_hex16(grid, x, y, value)` | Exactly four uppercase ASCII hex digits |

The shared [project font](/assets/fonts/README.md) covers printable ASCII32..126.
`tools/ui_font.py` provides `base_asset(characters, extra_glyphs=0)` for a
selected reusable font payload, plus `load_rows()`, `atlas(rows)` and `write_glyph` for
offline Native UI atlas generation; app manifests include `ui_font.SOURCES`
so glyph and provenance edits invalidate builds. The caller supplies an atlas
covering ASCII digits, A..F and minus. Reserve up
to five cells for unsigned decimal, six for signed decimal and four for hex.
The minimal decimal/signed/hex functions do not wrap, pad or erase trailing
cells; the caller clears changed fields or rebuilds its page. The fixed-width
unsigned field writes exactly `width` cells, including trailing skip255; zero
width writes nothing. Grid/atlas memory and GPU synchronization retain
their ordinary API contracts. No numeric string or framebuffer mirror is kept.

Plain strings use `UiGrid::write_text`; arbitrary IDs use `write_glyph` or the
existing rectangular `write`. These resource operations live in the GPU API,
while numeric formatting lives here. Native font art comes from the shared
project asset; glyph styles, icon allocation, menu selection, page layout and
explanatory text remain application responsibilities.
