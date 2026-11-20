---
id: bloxorz-marker-textures
status: implemented
last-verified: 2026-11-05
---

# Special-tile marker atlas

`tools/markers.py` rasterizes the reference eight-segment soft ring, heavy dark
square, crossed fragile cracks and splitter ring/two dots deterministically.
The original marble photograph supplies the underlying tile patterns. Reference channel
palettes and fixed fragile/split colors are baked into opaque RGB; there is no
transparent texture sampling. `markers.bin` is a single256×256 RGB565 column-macro
base layer at byte0x740000, exactly131072 bytes after packing/alignment. The main
stone/metal atlas is a separate256×256 base at0x680000. The resource producer checks disjoint
ranges and readback of the packed texels.

| Regions | Contents | Physical rectangles |
| --- | --- | --- |
| 0..31 | Eight soft channels × two states, marked top and matching sides | 32×32, first four rows |
| 32..47 | Eight heavy channels × inactive/active flat tops | Inactive32×32 at Y128; active16×16 at Y160 |
| 48..55 | Four fragile patterns, marked top and matching sides | Top48×48 at Y176; side32×32 at Y224 |
| 56..59 | Four splitter top patterns | 32×32 at X128..255, Y224 |

Rectangles do not overlap. Two texels of extruded edge padding surround every
pattern; the active heavy16×16 patches use one. UV endpoints are inner texel
centres. Quantized UVs are checked so both bilinear taps stay in the same physical
rectangle. Fragile marks have44×44 inner pixels; ordinary pattern cells have28×28.
Binary mask coverage is evaluated at final pixel centres, with both reflection
symmetries checked before compositing. The soft ring has reference inner/outer
radii.15/.25 with eight segments. Splitter uses that ring scaled by.8 (radii.12/.20)
and two.13-square dots centered at X±.16. Fragile crosses use two.66×.08 bars at
±45 degrees. A flat heavy top bakes the original.44-wide plate and.26-wide dark
square into the channel-tinted stone. These masks
are generated at their native resolution, without inclusive integer polygon
endpoints or a resized small intermediate. `marker-masks.png` and the per-slot
items in `assets.json` separate geometric coverage from the stone photograph. The underlying stone is resized directly
from its original photograph crop with Lanczos before the binary mark is composited.
The texture descriptor sets `full_mip=false`: existing filtering clamps to the
base layer, with explicit bilinear filtering, so minification never samples a mip combining neighbouring colors.
This avoids a larger isolated mip pyramid and needs no shader or LOD ABI change.

Soft marks are part of one textured tile mesh; the whole top keeps its small press
sink. Heavy buttons use a single flat marked tile with separate inactive/active
textures, without a raised body, pulse or sink. Heavy sides reuse the matching
channel/state soft-side regions. A marked fragile tile and its cracks disappear together at CRACK start,
and that cell stops receiving ground shadow immediately. Its retained batch
patches only the affected native indices and restores them on undo/reset.
Splitter marks replace the existing static top
face. Independent plain glyph meshes are absent. Walls, wells, opening panels,
bridge bodies and player cubes retain their geometric roles.

| Kind | Current native mesh layout |
| --- | --- |
| Soft | One three-face tile, selected by channel/state and visible X side |
| Heavy | One four-face tile (top, positive Z, both X), selected by channel/state with native culling |
| Fragile | Five faces per cell, packed into a separate retained spatial batch; variant UVs remain per vertex |
| Splitter | One marked top face plus the visible ordinary sides |

All geometry uses the existing encoded-color native renderer and Display path.
Opaque bridge tiles, ordinary dynamic tiles and marked soft bodies have
two fixed-camera variants: top, positive Z and either positive or negative X.
The application selects X from the cell centre and the stage eye; native backface
culling handles an eye inside that cell's X extent. The asset producer verifies
that every stage eye lies beyond the positive Z edge. Vertices, UVs and face
order retain their original values. Transparent hints retain all original faces
and their two-sided draw order; player cubes retain six faces while rotating.
Changing opaque face order can change isolated shared-edge coverage pixels.
Baking and RGB565 quantization can differ from the old unlit geometry glyphs;
there is no Web PBR or pixel-equivalence claim. Actual fixed-scene comparisons and
native DRAW/UI counts are retained with the task delivery.
