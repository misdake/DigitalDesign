# Fragment arithmetic

## Values arriving at shading

A fragment carries pixel position, S12F10 normal, D16 depth, and either RGB888
or two periodic UV16 fractions. The quad supplies a final unsigned U5F3 LOD.
Depth goes to ROP unchanged by shading. Global context supplies Lighting inputs,
specular RGB and alpha. The two material routes are:

| Meshlet attribute | Tint | Sample branch |
| --- | --- | --- |
| RGB | Interpolated RGB888 | Bypass, using white `(255,255,255)` |
| UV | White `(255,255,255)` | Filter the selected RGB565 texture |

`draw_unlit` independently replaces Lighting with `g=256,h=0`. A textured unlit
fragment still samples; an RGB lit fragment still runs Lighting. Bypasses use
defined constants and do not read an unwritten branch bank.

This chapter defines the selected pixel values. [Execution](/ip/gpu-v2/docs/execution.md)
explains the RAM, cache and arithmetic schedules that carry them.

## Lighting factors

Expand S12F10 normal codes left four to the S16F14 working port; normals arrive
unnormalized. Pixel-center NDC is also S16F14:

```text
ndc_x = RNE((2*x+1-400)*16384/400)
ndc_y = RNE((240-2*y-1)*16384/240)
ray = (ndc_x*ray_x, ndc_y*ray_y, ray_k)
N = normalize(normal); V = normalize(ray)
H = normalize(L+V)
d = clamp(dot(N,L),0,1)
g = min(Ia + Id*d, 511/256)
h = Id * clamp(dot(N,H),0,1)^s       # zero when NL <= 0
```

These equations state the Lighting function; the lookup quantization below is
part of its integer realization. g/h are unsigned 9-bit F8, with 256 representing one;
g ranges 0..511 and h0..256. Ia/Id lie in0..1. Ambient-only mode returns `(Ia,0)`.
The context's projection k lies in `[0.5,0.75]`, absolute ray scale is at most
0.75, and the quantized unit light has squared-length tolerance `32768/2^28`.
Context and opaque epoch remain immutable through drain.

Degenerate normal maximum below four Q14 codes, or half-vector maximum below 64
after rounded `(L+V)/2`, uses safe lookup evaluation followed by zero output.
Power-of-two common scaling preserves direction. N/V normalize component-wise;
H's reciprocal scales the scalar NH instead of forming three normalized H
component products.

### Reciprocal square root

Write positive squared magnitude as `2^e*t`, with `1<=t<2`. Exponent parity
selects one of two 64-segment pages. The compact 128-entry encoding stores Q13
14-bit base plus 4-bit residual; groups of eight share a 6-bit bias. Decode restores:

```text
base_q15 = base_q13 << 2
delta_q15 = (residual + bias[address>>3]) << 2
```

Base generation uses RNE. For segment fraction f in0..255, subtract
`floor(delta_q15*f/256)` from base_q15, then restore the exponent with the
integer shift `-floor(e/2)`. Component normalization floors the product to F14,
clamps to `[-16384,16384]`, and applies the degenerate-zero marker.

The selected kernel uses the CompensatedFloor policy. Its format-changing
operations obey this table; explicitly named shifts/slices keep their stated
bit semantics:

| Conversion | Rounding |
| --- | --- |
| Unsigned 18-bit F16 light product to unsigned 9-bit F8 | RNE |
| Other conversions to F8, except unsigned 16-bit F15 input | Half up |
| Unsigned 16-bit F15 to F8, and other precision reductions | Floor |
| Direct h9 interpolation | The explicit `(delta*f+32)>>6` rule below |

Dot products retain signed 34-bit F28 containers before the program's narrowing
boundaries. The format inventory is in [lighting-formats](/ip/gpu-v2/spec/lighting-formats.csv);
the [numerical program](/ip/gpu-v2/src/lighting/sim/counted.rs) records each typed
intermediate. These integer steps approximate the ideal normalization equations;
there is no general runtime divider.

### Specular power

Shininess codes 0..16 select
`4,5,6,7,8,10,12,14,16,20,24,28,32,40,48,56,64`.
The selected SmoothH9Reference-derived path forms h9 directly. A context-prepared
three-bit shift selects a cosine-distance grid. Each table entry is 9-bit base / 7-bit delta;
544 entries are used in an allocated 1024-entry ROM. Six fraction bits interpolate:

```text
value = base - ((delta*fraction + 32) >> 6)
```

The retained directional-intensity product follows this value. Nonpositive NL
and the far tail return zero; cosine 1 returns 256. The independent ordinary
power-table API is compared in [reference differences](/ip/gpu-v2/docs/references/README.md).

## Texture filtering

The input already contains biased/clamped LOD. No-mip textures use the base
layer; excessive LOD saturates at the coarsest available layer. Coordinates wrap
periodically. Nearest selects `floor(u*size)`; filtering uses `u*size-0.5` with
floor-Q8 fractions. Fine/coarse coordinates retain the signed half-scale
relationship, including the producer's padding at small levels.

UNORM9 tap weights sum exactly 511. Bilinear splitting proceeds row then column;
trilinear splitting first divides the fine/coarse parents and folds them into
the tap weights. U5F3's eight fractional codes give coarse parent weights
`0,64,128,192,256,319,383,447`. There is no additional final mip-color lerp.

For parent weight p and Q8 fraction f, use
`high=floor(p*f/256)`, `low=p-high`. Split the mip parent vertically into
top/bottom with fy, then each row horizontally into left/right with fx. Tap
order is top-left, top-right, bottom-left, bottom-right. Nearest assigns the
entire parent to its first tap. This fixes which child receives the remainder;
for p=511 and f=128, the children are 256 and 255.

Group4 names one tile's tap group: taps in other tiles have zero coefficients.
Zero-only groups disappear; first/last markers are rebuilt. A sample's groups
remain contiguous in first-tap order, with the finer mip first. This grouping
changes transport, not the conserved coefficients. Physical asset addresses
are defined once in [formats](/ip/gpu-v2/docs/formats.md#texture-assets).

## Selected Color accumulation

Expand RGB565 texels by bit replication. For each 9-bit-weight × 8-bit-channel tap
product p, form `(p>>9)+p[8]`, a half-up right shift by 9. Store each two-tap pair
sum in8 bits. Place pair values in the low 8 bits of separate 16-bit lanes of a
guarded 54-bit accumulator. The first group starts the sample accumulator; later
groups add. Expose each lane's low 8 bits, without further normalization or clamp.

Conserved weights do not change this into exact division by 511: rounding occurs
per tap, then the stated pair/accumulator bit widths apply. The selected Color
and generic exact-normalization Color are distinct arithmetic profiles.

## Final RGBA

Once the required branches complete, Final combines each channel:

```text
p = tint8 * texture8
t = p + 128
base = (t + (t >> 8)) >> 8
color8 = min(255, floor(base*g/256) + floor(specular8*h/256))
alpha8 = global alpha
```

The base step is exact nearest tint×texture/255. The diffuse and specular terms
truncate separately before addition and saturation. Alpha comes from context,
not the texture. RGB565 packing, alpha quantization and depth comparison happen
later in [framebuffer](/ip/gpu-v2/docs/framebuffer.md#pixel-update).

The [reference difference table](/ip/gpu-v2/docs/references/README.md) identifies which APIs use
RNE of the combined Final sum or exact Color normalization. Their outputs can
be compared only with the corresponding profile selected explicitly.

Implementation anchors: [Lighting ports](/ip/gpu-v2/src/lighting/ports.rs),
[selected Lighting lowering](/ip/gpu-v2/src/system/pixel/postquad_ii4.rs),
[Sampling](/ip/gpu-v2/src/texture/rtl/direct_lod.rs),
[Color](/ip/gpu-v2/src/system/pixel/vliw_color_direct.v),
[Final](/ip/gpu-v2/src/system/pixel/vliw_backend_final.v).
