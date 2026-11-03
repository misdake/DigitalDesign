---
id: bloxorz-marker-textures
status: implemented
last-verified: 2026-11-03
---

# Special-tile marker atlas

`tools/markers.py` rasterizes the reference eight-segment soft ring, heavy dark
square, crossed fragile cracks and splitter ring/two dots deterministically.
The original stone atlas supplies the underlying tile patterns. Reference channel
palettes and fixed fragile/split colors are baked into opaque RGB; there is no
transparent texture sampling. `markers.bin` is a single256×256 RGB565 column-macro
base layer at byte0x740000, exactly131072 bytes after packing/alignment. The original
512 atlas/mip chain at0x680000 is unchanged. The resource producer checks disjoint
ranges and readback of the packed texels.

| Regions | Contents | Physical rectangles |
| --- | --- | --- |
| 0..31 | Eight soft channels × two states, marked top and matching sides | 32×32, first four rows |
| 32..47 | Eight heavy tops and constant-color side patches | Marked32×32 at Y128; side8×8 at Y160 |
| 48..55 | Four fragile patterns, marked top and matching sides | Top48×48 at Y176; side32×32 at Y224 |
| 56..59 | Four splitter top patterns | 32×32 at X128..255, Y224 |

Rectangles do not overlap. Two texels of extruded edge padding surround every
pattern; the constant-color8×8 patches use one. UV endpoints are inner texel
centres. Quantized UVs are checked so both bilinear taps stay in the same physical
rectangle. Fragile marks have44×44 inner pixels; ordinary pattern cells have28×28.
The marker mask is drawn directly at this final resolution; it is not blurred
through a small Lanczos-resized intermediate. The underlying stone alone is
area-resampled before the binary mark is composited.
The texture descriptor sets `full_mip=false`: existing filtering clamps to the
base layer, so minification never samples a mip combining neighbouring colors.
This avoids a larger isolated mip pyramid and needs no shader or LOD ABI change.

Soft marks are part of one textured tile mesh; the whole top keeps its small press
sink. Heavy marks are part of the raised base top, retaining its body, pulse and
sink. A marked fragile tile and its cracks fall together, and the dropped cell
stops receiving ground shadow. Splitter marks replace the existing static top
face. Independent plain glyph meshes are absent. Walls, wells, opening panels,
bridge bodies and player cubes retain their geometric roles.

| Kind | Current native mesh layout |
| --- | --- |
| Soft | One five-face tile, selected by channel/state |
| Heavy | Original tile plus one six-face textured raised base |
| Fragile | One five-face tile per atlas variant |
| Splitter | One marked top face plus the visible ordinary sides |

All geometry uses the existing encoded-color native renderer and Display path.
Baking and RGB565 quantization can differ from the old unlit geometry glyphs;
there is no Web PBR or pixel-equivalence claim. Actual fixed-scene comparisons and
native DRAW/UI counts are retained with the task delivery.
