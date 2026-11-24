# Input and asset encodings

## Units and representation

All multibyte input objects and texels are little-endian. Host master addresses
are 22-bit halfword addresses (`byte_address/2`); local input RAM addresses are
64-bit qword indices (`byte_offset/8`). Texture and framebuffer layouts use byte
addresses before conversion at the memory wrapper. Never interchange these units.

| Local object | Qword address |
| --- | --- |
| Page p payload | `128*p`, capacity 128 qwords (1024 bytes) |
| Page p metadata | `256+4*p`, four qwords |
| Matrix/config area | Starts at 320, up to 64 qwords |
| Uniform base b | `16*b`; b is 20..23 |

There are two payload pages, for 2 KiB total. A descriptor always writes config
from local qword 320; `config_bytes` selects how much of that area to refresh.
Metadata selects the matrix block used by a meshlet.

## Meshlet payload

A meshlet contains at most64 vertices and96 triangles. All96-bit vertex records
precede a separately located index area (three triangles per64-bit word).
Metadata remains a separate object.
Payload start is 128-byte aligned; its byte length need only be a multiple of 8.

Each96-bit vertex is three u32 words:

| Word | Bits | Meaning |
| --- | --- | --- |
| 0 | X9:0, Y19:10, Z29:20 | Three unsigned delta10 position components |
| 1 | Nx9:0, Ny19:10, Nz29:20 | Three signed 10-bit normal components |
| 2 | All 32 | UV: U low 16, V high 16; RGB: R low 8, G15:8, B23:16 |

Top two bits of words 0/1 and RGB's high byte are producer-written zero. RGB and
UV are mutually exclusive. Decode uses metadata origin/quant_shift and the
normal transform described in [geometry](/ip/gpu-v2/docs/geometry.md#vertex-transform).

Billboard meshlets use the same V96 stride, position word, attribute word and
index encoding. Word 1 instead carries signed ten-bit offsets in bits9:0 and
19:10, with bits31:20 written zero. An offset code multiplied by 128 is the
signed F16 input to the billboard transform. The asset producer chooses a
world-unit scale and incorporates it in the corresponding uniform column.

One triangle is `i0 | (i1<<6) | (i2<<12)`. Three triangles occupy the low 54 bits
of one u64. High padding and unused tail indices are zero; each live index must
be below V count. Let `vwords=ceil(3*V/2)` and `twords=ceil(T/3)` in qwords.
The metadata T offset is at least vwords and must keep every fetched T word in
the page. Tight length is `8*(vwords+twords)` bytes. The maximum is
`768+256=1024` bytes. A V=3/T=1 example is [included in submission](/ip/gpu-v2/docs/submission.md#addressing-example).

## Meshlet metadata

| Qword | Bit fields |
| --- | --- |
| 0 | V count 6:0; T count 14:8; uniform base21:16; T qword offset 30:24; cull-negative 31 |
| 1 | Billboard mode 0 (0=Mesh, 1=Billboard); reserved-zero3:1; RGB mode 4 (0=UV, 1=RGB); sequence start 11:5; generation 15:12 |
| 2 | origin_x31:0; origin_y63:32 |
| 3 | origin_z31:0; quant_shift36:32; normal18 bit 40 |

All other metadata bits are producer-written zero. Nonempty T requires nonzero
V. Bits3:1 of word1 are reserved zero.
`quant_shift` controls local source position decode; T consumes V's F16 position
without a meshlet-wide shift.
`normal18` selects the normal quantization path for ordinary Mesh.

## Matrix block

Each uniform block contains 32 u32 words in 16 qwords. Matrix coefficients and
origins use signed 32-bit F16 values.

| u32 word index | Contents |
| --- | --- |
| 0..11 | Position 3 × 4 matrix, row-major: three coefficients then translation per row |
| 12..20 | Normal 3 × 3 matrix, row-major |
| 21 | F16 homogeneous constant 1 (`65536`), read by the Vertex bootstrap |
| 22 | Command profile: mesh uniform RGBA8888; R low8, A high8; white/opaque is `0xffffffff` |
| 23 | Reserved zero in the precomposed format |
| 24..29 | Billboard mode: direction pairs `(Ax, Bx, Ay, By, Az, Bz)` in projected coordinates. Written zero in a normal Mesh block |
| 30..31 | Reserved zero. The Billboard bootstrap reads word30 for its zero normal |

In the command profile, uniform RGB modulates the one selected vertex-color or
texture source, with exact nearest UNORM8 multiplication in Final. Alpha is
multiplied by filtered texture alpha in ROP; RGB vertices supply opaque alpha. No modulation
is encoded by the driver as white RGB and opaque alpha, with no enable
flag or hardware default selection. Non-command component APIs retain their
independent uniform contracts. `mesh_color_words` encodes words22/23.

### Resident View/World blocks

DRAW bit1 selects World20/21 plus the shared View23. These are distinct from
precomposed Instance128. DRAW bit2 selects the vertical Billboard basis; bit2
requires bit1. Ordinary Mesh ignores the facing selector.

| World u32 words | Contents |
| --- | --- |
| 0..11 | Source-world affine position3x4 |
| 12 | Q16.16 one (`65536`) |
| 13..14 | Q16.16 material ambient/directional responses, not multiplied by light |
| 15 | Zero |
| 16..24 | Source-world inverse-transpose normal3x3 |
| 25 | Zero |
| 26..31 | Material24 below |

World is two64-byte sections. Normal and Material share the second section;
its final32-byte block begins with the last normal coefficient and zero padding.
SET_RENDER's extended format skips those first8 bytes. A 128-byte World upload
uses four32-byte CPU LCOPY operations.

| View u32 words | Contents |
| --- | --- |
| 0..11 | Projected camera affine position3x4 |
| 12..20 | Reflected camera normal3x3 |
| 21 | Q16.16 one |
| 22..23 | Projected horizontal camera-right X/Y for vertical Billboard |
| 24..27 | Scene16: far, view-light XYZ and projection rays; same first16 bytes as NativeGlobal64 |
| 28..29 | Q16.16 ambient/directional scene intensities |
| 30 | Projected horizontal camera-right Z |
| 31 | Billboard camera-plane focal scale |

View's tail32 bytes can be refreshed independently. Its two Billboard coefficients
at words30/31 must survive a lighting-only update.

| Material u32 word (World index) | Encoding |
| --- | --- |
| 0 (26) | Texture descriptor, identical to NativeRender32 word0 |
| 1 (27) | Specular RGB low24, high8 zero |
| 2 (28) | Depth func2:0, depth-write3, blend4, unlit5, signed LOD bias15:8; other bits zero |
| 3 (29) | Reserved zero |
| 4 (30) | Uniform RGBA8888 |
| 5 (31) | Lighting mode1:0, shininess code6:2, epoch31:16; other bits zero |

Geometry composes into uniform22 and leaves World/View immutable. Ordinary
Mesh writes derived position0..11, normal12..20, one21 and biased light-response
scratch24/25. The rounded Q8 windows update the existing CP lighting registers.
Billboard writes position, one and expanded direction pairs24..29; it is unlit.
The original Vertex/bounds/Billboard consumers keep their old coefficient layout.
Other derived words are not initialized. A later precomposed draw selecting22
must first reload the complete block, whose validity is cleared by a World draw.

The [preparation calendar](/ip/gpu-v2/docs/execution.md#resident-matrix-preparation)
defines DMA priority, ownership and the transition back to ordinary Vertex.

## Native command objects

The [command profile](/ip/gpu-v2/docs/commands.md) consumes little-endian native
objects from driver staging. Instance128 uses the matrix block above, including
the mandatory F16 constant at word 21.

NativeGlobal64 is sixteen u32 words:

| Word | Encoding |
| --- | --- |
| 0 | F16 `far_z`, strictly between zero and 200 |
| 1 | Signed F14 light_x low16, light_y high16 |
| 2 | Signed light_z low16, ray_x high16 |
| 3 | Signed ray_y low16, ray_k high16 |
| 4 | ambient bits8:0; directional bits24:16; each 0..256; other bits zero |
| 5 | mode bits1:0; shininess code bits6:2 (0..16); epoch bits31:16; other bits zero |
| 6..15 | Zero |

NativeRender32 is eight u32 words:

| Word | Encoding |
| --- | --- |
| 0 | Current texture: base byte address/128 bits15:0; size_log2 bits19:16; full-mip20; filter bits22:21; fixed400x240 view bit23; RGBA4444 bit24 (zero selects RGB565); POT ClampToEdge bit25 (zero selects Repeat); bits31:26 zero |
| 1 | RGB888 specular low24; high8 zero (alpha comes from mesh uniform and texture) |
| 2 | depth_func bits2:0, depth_write3, blend4, unlit5; other bits zero |
| 3 | Signed F4 lod_bias bits7:0; other bits zero |
| 4..7 | Zero |

These are the current numerical pins, not floating-point source parameters.
Their arithmetic and direction requirements remain in
[fragment arithmetic](/ip/gpu-v2/docs/fragment-math.md). The host chooses a
legal column POT RGB565 or RGBA4444 texture, exponent0..10 and filter0/1/2, or the
fixed400x240 view described in [framebuffer views](/ip/gpu-v2/docs/framebuffer-texture-view.md).
Both use 256-byte base alignment and the same physical cache backend. POT uses
normalized UV Repeat or ClampToEdge and the selected mip level; fixed views use signed18 UV
pre-clamp, true400/240 scaling, nearest/bilinear and base-only data. Exponent9
on a fixed view is an encoding carrier, never a512-square footprint.
NativeRender32 shape, format and address mode are part of state equality and the
locked CP binding. They are global alongside mip/filter and change only after
draining prior work. Fixed views remain RGB565 and reject bits24/25; their
existing implicit clamp is unchanged. ClampToEdge clamps signed18 input UV to
0..65535 before narrowing, then handles endpoints independently at each sampled
mip. Nearest uses floor(u*N); filtered coordinates use u*N-0.5 clamped to
0..N-1. Adjacent wrapped taps retain the original bank access pattern; an endpoint
fraction of zero gives the wrapped neighbor zero weight. LOD uses the original
UV differences. POT Repeat remains the default.

Production POT mip levels use16x16 16-bit column macros,512 bytes each.
RGBA4444 stores R15:12, G11:8, B7:4 and straight alpha A3:0. Both formats
support the same full mip chain and all three filters; raw cache lines carry
no format tag and do not expand texels.
For level n, pitch is `max(n-4,0)` and
`address = level_base + 512*(((x>>4)<<pitch)+(y>>4)) + 32*(y&15) + 2*(x&15)`.
Every small level still occupies512 bytes, with periodic padding. Full chains
are smallest first; level prefixes in128-byte sectors are
`0,4,8,12,16,20,36,100,356,1380,5476`. A base-only level has prefix0.
The resident line is16x8/256B and requires two successful128B child reads.
`systems/cpu-v3-tang-nano-20k/tools/gpu_texture_layout.py` is the asset packer.


## Command meshlet record

The explicit command profile uses independently addressed 128-byte-aligned
records. Bytes0..31 are the unchanged metadata; bytes32..127 are allocation
padding and are not read. Payload begins at byte128 and contains the same
96-bit vertices and three-triangle/64-bit index words, padded to1..8 complete128-byte sectors. A command
carries the record address and payload sector count. There is no GPU Mesh
header, directory, implicit loop or selected first/count range.

The source allocation includes the whole prefix and all padded payload sectors
below 8 MiB. The driver validates counts, indices, uniform selection and the
payload's complete vertex/index coverage before submission; the existing
meshlet controller retains its runtime header and reference checks. The DMA
completion only publishes a completely transferred page; it does not certify
arbitrary asset bytes as valid geometry.

Instance128 is copied directly to one of the existing uniform blocks320..383,
using an aligned 128-byte local copy. Metadata chooses uniform_base20..23.
Changing a uniform happens between V/T invocations. Global/render replacement
requires the separate full-render wait described in [commands](/ip/gpu-v2/docs/commands.md).

Translation is a coefficient in the fourth column, not an extra source vertex
component. A block's position result has three components; there is no implied
fourth clip-Z output or once-rounded 4 × 4 transform.

## Interpolation table

FPGA configuration initializes every index j=0..255 with a 36-bit word:

```text
seed[j]  = RNE(2^25 / (256+j))                  # j=0..256 for generation
slope[j] = seed[j] - seed[j+1]
log[k]   = RNE(256 * log2(1+k/64))              # k=0..63
word[j]  = slope[j] | (seed[j]<<9) | (log[j>>2]<<27)
```

Bits 8:0 hold 9-bit slope, bits 26:9 18-bit seed, bits 34:27 8-bit log; bit 35 is zero. Generation
uses mathematical log2; runtime performs table lookup. The exporter generates these
constants once for both native INIT_RAM parameters and portable simulation.
The existing 512x36 BSRAM holds this immutable table at rows 0..255 and four
mutable H rows at 256..259. Its only write address selects those four H rows;
there is no runtime numerical-table input or command. Reset clears Q and pipeline
state, not the RAM contents. Table contents survive ordinary reset.

## Texture assets

Independent generic numerical sampler fixtures, outside the production native
command format above, use up to16
factory-supplied slots containing validity, aligned byte base, full-mip
flag and size exponent 0..10. Assets are square power-of-two RAW RGB565. There is
no texture alpha. Each 8 × 8 tile occupies 128 bytes, with row-major little-endian
texels. Levels smaller than 8 × 8 still contain periodic 8 × 8 padding supplied
by the producer. Full mip chains are ordered smallest to largest.

Layer offsets in 128-byte tiles, for exponent 0..10, are:

| Exponent | 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | 10 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Offset | 0 | 1 | 2 | 3 | 4 | 8 | 24 | 88 | 344 | 1368 | 5464 |

For exponent n, `side=1<<max(n-3,0)` and
`byte_address=base+128*(prefix+tile_y*side+tile_x)`. A texture without mips uses
prefix 0 for its base layer. The allocation must fit both the 32-bit byte-address
range and actual system memory. Filtering coordinates and coefficients are in
[fragment arithmetic](/ip/gpu-v2/docs/fragment-math.md#texture-filtering); physical cache mapping
is in [execution](/ip/gpu-v2/docs/execution.md#texture-cache).

The autonomous command profile replaces that registry with NativeRender32's
single current descriptor. SET_RENDER never invalidates texture data. Physical
sector tags retain immutable texture data across descriptor changes. After
WAIT(all), the driver issues TEX_INVALIDATE when making an in-place texture
update visible; it also owns any CPU/external-memory coherence. A different
descriptor, filter, bias or Lighting alone does not require invalidation.

Framebuffer color/depth plane layout has its own owner in
[framebuffer](/ip/gpu-v2/docs/framebuffer.md#surface-layout), where pixel writes and tile transfers
can be read together.

Implementation anchors: [metadata decode](/ip/gpu-v2/src/frontend/vliw/meshlet.v),
[input decoder](/ip/gpu-v2/src/frontend/vliw/frontend.v),
[integer/table generator](/ip/gpu-v2/examples/vliw_integration/reference/scene/oracle.py),
[texture asset ports](/ip/gpu-v2/src/texture/ports.rs).
