# Bloxorz input assets

The generated native mesh catalog contains two visible-side variants for opaque
dynamic tiles, bridges and soft markers. Original channel/state indices
form the positive-X prefix; the negative-X family follows. The producer preserves
packed vertex/UV values, remaps indices and enables native backface culling.
Transparent hints preserve their full original two-sided geometry. See the
[marker layout](/systems/cpu-v3-tang-nano-20k/examples/game_bloxorz/docs/markers.md)
for camera selection and coverage limits. Consumers resolve groups through the
generated catalog rather than relying on literal mesh indices.
Heavy buttons have a single four-face native tile per channel/state; the flat
marked top and reused side-atlas regions replace the separate raised body.

`background-rgb565-column16.bin` is the original ordered-dither background,
204800 bytes, SHA256
`5f4be20a8bf7708d54ff2bde923210964c147eeb53c43c0b2e6ded71f9dc8047`.
It retains the accepted asset's original bytes rather than re-encoding a PNG.

The visible image is 400x240, with 16 padded rows in the physical 400x256 plane.
For pixel coordinates (x,y), its little-endian RGB565 offset is
`2*((x>>4)*4096+(y>>4)*256+(y&15)*16+(x&15))`. The source uses sRGB-coded
components and the application selects ordinary `DisplayFormat::Rgb565`.
Applying the linear-RGB565 Display lookup would apply an extra transfer curve.

The producer reserves byte 0x580000 for this immutable source and excludes the
0x600000 offscreen color allocation and the shared scene depth at0x480000. Every scene restoration uses
the current public `copy_color` when building the static scene, with independent
depth Clear. Runtime-only static color and depth reserve respectively
`[0x500000,0x540000)` and `[0x540000,0x580000)`. The producer records these
reservations separately from boot payloads and rejects overlap with assets;
neither plane is a second Flash image. Each ordinary restoration copies their
204800-byte physical spans into the selected color target and shared work depth,
then uses color/depth Load. Allocation tails remain untouched.
SELECT toggles this intentional gradient and the Web pixel-mode solid #2c2620.
The main and marker textures start directly from the original1024×1024 photos in
`source/`, using the source identities/crops described in [base textures](/systems/cpu-v3-tang-nano-20k/examples/game_bloxorz/docs/textures.md).
The photos are declared CC0 by the original BloxorzClone builder, from Poly Haven
[marble_01](https://polyhaven.com/a/marble_01) and
[rusty_metal_sheet](https://polyhaven.com/a/rusty_metal_sheet).
Procedural marker masks and application-only HUD glyphs remain native sources;
their PNG previews are output artifacts. The old512 atlas is not a build source
for these materials. Source SHA256 values and the declared license are emitted
in the asset manifest from `source_photos.py`.

`web-shadow-reference.json` is an independent, generated Web geometry fixture:
actual world matrices and clipped receiver polygons for standing, rising,
tipping and released falling. Its source-file hashes bind the reference input;
it does not contain native shader output or a second level/Flash image.

## Native HUD font

The HUD uses the shared [project font](/assets/fonts/README.md): native 8×16
VonwaonBitmap16pxLite 1.02 ASCII32..126. The canonical glyphs, original source
identity and extraction tool live in `/assets/fonts`, not in this application.
The shared system packer preserves native bearings, baseline14 and advance8;
there is no rescaling or added line spacing.

The producer's `hud-font-atlas-1x1.png` decodes all256 cells of the actual
65536-byte Native UI atlas, including row-bank XOR, into a128×256 RGBA preview.
White65535 is opaque and zero transparent. The game atlas is byte-identical to
the original Bloxorz HUD font atlas.


## Goal carrier assets

`tools/goal_carrier.py`, invoked by the normal asset producer, generates48 native
annulus meshes and their matching hatch tops, plus `goal_carrier.bin`: four
128x128 RGB565 base-only textures,128KiB total. It resizes the original stone
photo crops directly into final planar cells and adds a flat red inlay, with two-pixel gutters. Unused red center texels
are not a fake hole: the center is removed from the mesh until it closes.
The native mesh directory and all records share the existing mesh resource;
resource disjointness and its0x4f0000 upper boundary are checked. The archive's
per-level applicability flag preserves moving neighbor tiles through fallback.
See [goal behavior](/systems/cpu-v3-tang-nano-20k/examples/game_bloxorz/docs/transitions.md#planar-goal-and-live-aperture)
for live selection, layout and visual tradeoffs. No extra source font or external
Web assets are required by the carrier producer.


`tools/ground_batches.py` adds a sorted static face schedule and24 tiny immutable
quad templates to the existing level archive. They preserve the original face
list and carry native corner signs, normal bits and UV bits; actual stage-wide
vertex/index arrays are generated by RCC into heap storage. The independent
`tools/check_ground_batches.py` decodes the original native mesh resource to
produce the test reference rather than reading generated heap records.

CPU readers use generated typed `BloxLevelHeader` records, packed palettes and
the native quad templates in the static region. These values are derived from
the same archive, rather than maintained as a second source. Compact rules and
the current level's contiguous face lists are copied in blocks through DSEG3;
only page crossings change the mapping, and the original page is restored at
the end. Face lists remain in the application's bounded `LevelGeometryCache`.
See [archive and CPU storage](/systems/cpu-v3-tang-nano-20k/examples/game_bloxorz/docs/transitions.md)
for layout and lifetime.
