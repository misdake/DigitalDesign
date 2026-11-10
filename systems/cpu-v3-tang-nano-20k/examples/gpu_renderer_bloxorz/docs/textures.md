---
id: bloxorz-base-textures
status: implemented
last-verified: 2026-11-10
---

# Base textures and source ownership

Bloxorz uses base-only bilinear textures for its bounded game cameras. It does
not change the shared GPU's mip support. `TextureFilter::Bilinear` encodes1;
`full_mip=false` clamps analytic LOD to0. Trilinear code2 at LOD0 has a zero
coarse-level weight and produces the same sampling result, but the application
uses code1 explicitly. Shadows and the fullscreen presentation pass retain their
own nearest filtering. The HUD is an independent native binary glyph lookup;
the background is a raw color copy. Neither is a 3D atlas mip allocation.

The main atlas, marker atlas and independent goal-carrier views explicitly use
`TextureAddressMode::ClampToEdge` through the application-owned
`texture_views::clamped_texture_view` helper. Carrier replacements reapply that
policy. Their RGB565 storage, resource bytes and filter selection are unchanged.
Clamp bounds the complete texture; the internal atlas slot rings and UV insets
remain necessary to isolate neighboring items. RTT color-buffer views retain
their existing implicit fixed-shape clamp.

| Resource | Physical byte base | Logical size / bytes | Content and borders |
| --- | --- | --- | --- |
| Main stone/metal | 0x680000 | 256×256 /131072 | Four64×64 floor slots:56×56 content+4 texel ring; three128×128 player slots:120×120 content+4 ring |
| Special markers | 0x740000 | 256×256 /131072 | 60 disjoint rectangles; two-texel rings, one on constant8×8 patches; see markers |
| Goal carrier phases | 0x760000 | Four independent128×128 /131072 total | Each124×124 planar pattern+2 texel outer ring |
| HUD lookup | 0x4f0000 | 65536 bytes | Existing8×16 binary glyph format and row-bank XOR |

Main slot locations preserve the original normalized seven-region layout.
`atlas_borders.py` computes UV endpoints at the first/last content texel centers,
then `web_scene.py` quantizes them to UNORM16 with its existing V flip. Material
and batch grouping are unchanged. Both bilinear coordinates remain in the
owning physical slot, including a one-Q16-unit guard around the quantized range.
The edge ring copies that item's nearest content pixel; no neighboring slot is
read to produce it. Pixel coordinates use the existing column-macro packing.
There are no smaller mip levels, offsets or repeated mip tails in these files.

The main256 texture retains enough source detail for the largest fixed-camera
floor edges and player faces. A maximum-axis gradient LOD alone does not bound
magnification along the other axis, so it is not used to justify a small base.
The producer also accepts explicit main64/128 and marker128 diagnostic sizes.
All sizes retain the same native format, normalized slot layout and matching
UVs/descriptors. One base-only bilinear filter can still alias on foreshortened
surfaces; this does not establish temporal antialiasing or hardware performance.

The two256 atlases occupy262144 bytes in total. Main ends at0x6a0000 and markers
end exactly at0x760000, where the separate goal-carrier allocation begins.

## Original photographs and generation

`assets/source/marble_01.jpg` and `rusty_metal_sheet.jpg` are the original1024×1024
photographs from the user's BloxorzClone reference. `source_photos.py` binds their
SHA256 identities and records their Poly Haven source URLs and the reference
builder's CC0 declaration. Its crop, grayscale/rim and metal tone operations
preserve that builder's definitions. Logical floor variants include the original
UV V-flip; they are not silently reordered to match the atlas preview.

Each material starts from its photograph crop and tone, then performs one
Lanczos resize directly to the final content dimensions. Marker masks are drawn
at their final native content size after tinting; no marker preview is resized.
The producer extrudes the final edges, quantizes once to RGB565 and packs columns.
It neither downsamples the old512 atlas nor uses an old mip/quantized image as
a material source. All component values retain the reference encoded color space;
no extra gamma conversion or Display transfer curve is introduced.

Goal carrier phases independently resize the original stone crops into their
final41/42-pixel planar cells and bake the existing tint/lighting scale. Their
opaque center is covered or removed by the existing aperture geometry. They do
not resample the quantized main atlas. The separately generated hatch samples
the ordinary stone base with cropped UVs. Geometry, depth, retirement and cache
publication contracts are unchanged.

## Verification tools

`check_atlas_borders.py <asset-directory>` independently decodes packed rings,
checks quantized UV/guard footprints in all67 main/marker slots and analyzes all33
actual level dimensions/cameras. Floating projected tile-edge density is reported
as a bound for static edges, not a temporal or foreshortened-triangle guarantee.

`atlas_trace.py` builds an observation-only copy of selected GPU functional
sources. It reports actual LOD and nonzero weighted reads separately for valid
covered fragments and fragments passing the current ROP depth comparison.
Base-only materials additionally report a diagnostic gradient LOD computed with
the mip clamp lifted only in that observation calculation. It does not change
the actual sampler LOD. Counts include overdraw; they are not unique visible
pixels or elapsed cycles. Every observed submission is also run through the
public Executor and all physical outputs must match. Run this observer serially
with `RAYON_NUM_THREADS=1`; it records generated/source identities and bounded
per-submission work. It does not edit shared GPU source or establish board timing.
